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

fn refresh(sys: &mut System) {
    sys.refresh_processes(ProcessesToUpdate::All, true);
}

pub fn server_status(cfg: &Config) -> ServerStatus {
    let mut sys = System::new();
    refresh(&mut sys);
    let marker = format!("modeldesk-{}", cfg.port);
    for (_, proc_) in sys.processes() {
        let cmd = cmd_line(proc_);
        let has_marker = cmd.contains(&marker);
        let is_provider = cmd.contains("mlx-dspark")
            || cmd.contains("mlx_lm.server")
            || cmd.contains("llama-server")
            || cmd.contains("ollama");
        if has_marker || (is_provider && cmd.contains(&cfg.port.to_string())) {
            return ServerStatus::Running {
                pid: proc_.pid().as_u32(),
                port: cfg.port,
            };
        }
    }
    if port_listening(cfg.port) {
        return ServerStatus::Running {
            pid: 0,
            port: cfg.port,
        };
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

fn marker_arg(cfg: &Config) -> String {
    format!("modeldesk-{}", cfg.port)
}

pub async fn start(cfg: &Config, entry: &ModelEntry, foreground: bool) -> Result<()> {
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
    let bin = provider.binary().map_err(|_| ModeldeskError::ProviderMissing {
        provider: entry.provider.clone(),
        hint: String::new(),
    })?;
    let args = provider.start_args(entry, &cfg.host, cfg.port)?;

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
    let owned_pid: Option<u32> = fs::read_to_string(platform::state_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get("pid").and_then(|p| p.as_u64()).map(|p| p as u32));
    let mut sys = System::new();
    refresh(&mut sys);
    let marker = marker_arg(cfg);
    let mut killed = false;
    let mut foreign: Vec<String> = Vec::new();
    for (pid, proc_) in sys.processes() {
        let cmd = cmd_line(proc_);
        let is_provider = cmd.contains("mlx-dspark")
            || cmd.contains("mlx_lm.server")
            || cmd.contains("llama-server")
            || cmd.contains("ollama");
        if !(is_provider && cmd.contains(&cfg.port.to_string())) {
            continue;
        }
        let owned = owned_pid == Some(pid.as_u32()) || cmd.contains(&marker);
        if owned {
            if proc_.kill() {
                killed = true;
            }
        } else {
            foreign.push(format!("{} (pid {})", proc_.name().to_string_lossy(), pid));
        }
    }
    let state_path = platform::state_path();
    if state_path.exists() {
        let _ = fs::remove_file(&state_path);
    }
    for _ in 0..10 {
        if matches!(server_status(cfg), ServerStatus::Stopped) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    match server_status(cfg) {
        ServerStatus::Stopped => Ok(killed),
        ServerStatus::Running { .. } => {
            let holder = if foreign.is_empty() {
                compat::port_holder(cfg.port).unwrap_or_else(|| "another process".to_string())
            } else {
                foreign.join(", ")
            };
            Err(ModeldeskError::PortBusy {
                port: cfg.port,
                holder,
            }
            .into())
        }
    }
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
