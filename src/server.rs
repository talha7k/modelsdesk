use anyhow::{Context, Result};
use std::fs;
use std::process::Stdio;
use sysinfo::{ProcessesToUpdate, System};

use crate::compat;
use crate::config::{Config, ModelEntry};
use crate::error::ModeldeskError;
use crate::platform;
use crate::providers::{self, ServerStatus};

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("http client")
}

fn cmd_line(proc_: &sysinfo::Process) -> String {
    proc_
        .cmd()
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_provider_cmd(cmd: &str) -> bool {
    cmd.contains("mlx-dspark")
        || cmd.contains("mlx_lm.server")
        || cmd.contains("llama-server")
        || cmd.contains("ollama")
}

/// Pids listening on a port via lsof (sysinfo misses hardened macOS processes).
#[cfg(unix)]
pub fn port_pids(port: u16) -> Vec<u32> {
    std::process::Command::new("lsof")
        .args(["-ti", &format!(":{port}")])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| {
            s.split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(windows)]
pub fn port_pids(_port: u16) -> Vec<u32> {
    Vec::new()
}

/// Full command line for a pid via ps (sees what sysinfo misses).
#[cfg(unix)]
fn pid_cmdline(pid: u32) -> Option<String> {
    std::process::Command::new("ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(windows)]
fn pid_cmdline(_pid: u32) -> Option<String> {
    None
}

fn refresh(sys: &mut System) {
    sys.refresh_processes(ProcessesToUpdate::All, true);
}

pub fn server_status(cfg: &Config) -> ServerStatus {
    let mut sys = System::new();
    refresh(&mut sys);
    for (pid, proc_) in sys.processes() {
        let cmd = cmd_line(proc_);
        if is_provider_cmd(&cmd) && cmd.contains(&cfg.port.to_string()) {
            return ServerStatus::Running {
                pid: pid.as_u32(),
                port: cfg.port,
            };
        }
    }
    // sysinfo misses hardened macOS processes — resolve via lsof + ps.
    for pid in port_pids(cfg.port) {
        if let Some(cmd) = pid_cmdline(pid) {
            if is_provider_cmd(&cmd) {
                return ServerStatus::Running { pid, port: cfg.port };
            }
        }
    }
    if port_listening(cfg.port) {
        return ServerStatus::Running { pid: 0, port: cfg.port };
    }
    ServerStatus::Stopped
}

#[cfg(unix)]
fn port_listening(port: u16) -> bool {
    use std::net::TcpListener;
    TcpListener::bind(format!("127.0.0.1:{port}")).is_err()
}

#[cfg(windows)]
fn port_listening(port: u16) -> bool {
    use std::net::TcpListener;
    TcpListener::bind(format!("127.0.0.1:{port}")).is_err()
}

pub async fn start(cfg: &Config, entry: &ModelEntry, foreground: bool) -> Result<()> {
    start_with_options(cfg, entry, foreground, true).await
}

/// Launch the backend. When `ensure` is true (default path), the runtime is
/// installed when missing, the version is gated, and weights are fetched —
/// so `start` self-heals instead of failing on environment gaps.
pub async fn start_with_options(
    cfg: &Config,
    entry: &ModelEntry,
    foreground: bool,
    ensure: bool,
) -> Result<()> {
    if matches!(server_status(cfg), ServerStatus::Running { .. }) {
        let holder = compat::port_holder(cfg.port).unwrap_or_else(|| "another process".to_string());
        return Err(ModeldeskError::PortBusy {
            port: cfg.port,
            holder,
        }
        .into());
    }
    let report = compat::check(entry, cfg.port)?;
    for note in &report.notes {
        eprintln!("note: {note}");
    }
    let provider = providers::for_entry(entry);
    if ensure {
        ensure_runtime(&*provider, entry).await?;
    }
    let bin = provider.binary().map_err(|_| ModeldeskError::ProviderMissing {
        provider: entry.provider.clone(),
        hint: String::new(),
    })?;
    // Resolved weights (GGUF downloads) override the repo id at launch.
    let mut launch_entry = entry.clone();
    if ensure && entry.provider == "llama-cpp" {
        match provider.fetch_weights(entry).await {
            Ok(Some(path)) => {
                launch_entry.repo = path.to_string_lossy().to_string();
            }
            Ok(None) => {}
            Err(e) => {
                return Err(ModeldeskError::Incompatible {
                    repo: entry.repo.clone(),
                    provider: entry.provider.clone(),
                    reason: format!("weight fetch failed: {e:#}"),
                }
                .into());
            }
        }
    }
    let args = provider.start_args(&launch_entry, &cfg.host, cfg.port)?;

    let log_path = platform::log_path();
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if foreground {
        let status = tokio::process::Command::new(&bin)
            .args(&args)
            .env("MODELDESK_PORT", cfg.port.to_string())
            .status()
            .await
            .with_context(|| format!("spawning {}", bin.display()))?;
        if !status.success() {
            anyhow::bail!("server exited with {status}");
        }
        return Ok(());
    }

    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("opening {}", log_path.display()))?;
    let log_err = log_file.try_clone()?;

    let mut child = tokio::process::Command::new(&bin)
        .args(&args)
        .env("MODELDESK_PORT", cfg.port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_err))
        .spawn()
        .with_context(|| format!("spawning {}", bin.display()))?;

    let pid = child.id().context("no child pid")?;
    let state = serde_json::json!({ "pid": pid, "port": cfg.port });
    let state_path = platform::state_path();
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&state_path, serde_json::to_string_pretty(&state)?)?;

    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

pub async fn stop(cfg: &Config) -> Result<bool> {
    // A backend binary on OUR port is ours — regardless of who launched it.
    // A foreign process (Expo, another tool) can never match: it neither runs
    // a provider binary nor serves our API on our port.
    let mut sys = System::new();
    refresh(&mut sys);
    let port_str = cfg.port.to_string();
    let mut killed = false;
    for (_, proc_) in sys.processes() {
        let cmd = cmd_line(proc_);
        if is_provider_cmd(&cmd) && cmd.contains(&port_str) && proc_.kill() {
            killed = true;
        }
    }
    // sysinfo misses hardened macOS processes — fall back to kill by pid.
    for pid in port_pids(cfg.port) {
        let cmd = pid_cmdline(pid).unwrap_or_default();
        if is_provider_cmd(&cmd) {
            killed = libc_kill(pid) || killed;
        }
    }
    let state_path = platform::state_path();
    if state_path.exists() {
        let _ = fs::remove_file(&state_path);
    }
    for _ in 0..15 {
        if matches!(server_status(cfg), ServerStatus::Stopped) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    match server_status(cfg) {
        ServerStatus::Stopped => Ok(killed),
        ServerStatus::Running { .. } => {
            let holder =
                compat::port_holder(cfg.port).unwrap_or_else(|| "another process".to_string());
            Err(ModeldeskError::PortBusy {
                port: cfg.port,
                holder,
            }
            .into())
        }
    }
}

#[cfg(unix)]
fn libc_kill(pid: u32) -> bool {
    // SIGTERM, no libc crate needed.
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid as i32, 15) == 0 }
}

#[cfg(windows)]
fn libc_kill(_pid: u32) -> bool {
    false
}

pub fn tail_log(lines: usize) -> Result<String> {
    let path = platform::log_path();
    if !path.exists() {
        return Ok(String::new());
    }
    let raw = fs::read_to_string(&path)?;
    let all: Vec<&str> = raw.lines().collect();
    let start = all.len().saturating_sub(lines);
    Ok(all[start..].join("\n"))
}

/// Ensure the backend is installed, new enough, and (for GGUF) has weights.
/// Installs automatically unless `--no-install` / `MODELDESK_NO_INSTALL=1`.
pub async fn ensure_runtime(
    provider: &dyn providers::Provider,
    entry: &ModelEntry,
) -> Result<()> {
    use crate::runtime_env::Env;
    use crate::runtime_install as ri;

    let env = Env::detect();
    ri::ensure_arch_ok(provider.id(), &env)?;

    let installed = provider.binary().is_ok();
    if !installed {
        if ri::no_install_mode() {
            return Err(ModeldeskError::ProviderMissing {
                provider: provider.id().to_string(),
                hint: String::new(),
            }
            .into());
        }
        let plan = ri::plan(provider.id(), &env)?;
        println!(
            "{} backend '{}' missing — installing via {}…",
            crate::output::cyan("●"),
            provider.id(),
            plan.steps
                .iter()
                .map(|s| s.describe())
                .collect::<Vec<_>>()
                .join(", ")
        );
        ri::run_plan(&plan)?;
    }

    match provider.installed_version() {
        Ok(Some(v)) => {
            if let Some(min) = provider.min_version() {
                if !providers::version_meets(&v, min) {
                    eprintln!(
                        "{} backend '{}' is {v}, want ≥ {}.{}.{}. Continuing anyway — upgrade when you can.",
                        crate::output::warn("old"),
                        provider.id(),
                        min.0,
                        min.1,
                        min.2
                    );
                }
            }
        }
        Ok(None) => {
            eprintln!(
                "{}",
                crate::output::dim(&format!(
                    "could not determine {} version — continuing",
                    provider.id()
                ))
            );
        }
        Err(e) => {
            eprintln!(
                "{}",
                crate::output::dim(&format!("version probe failed: {e:#}"))
            );
        }
    }
    let _ = entry;
    Ok(())
}
