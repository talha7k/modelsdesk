use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::output;
use crate::runtime_env::{Arch, Env, LinuxDistro, Os, PackageManager};

#[derive(Debug, Clone)]
pub struct InstallStep {
    pub manager: PackageManager,
    pub manager_label: &'static str,
    pub args: Vec<String>,
}

impl InstallStep {
    pub fn describe(&self) -> String {
        format!("{} {}", self.manager_label, self.args.join(" "))
    }
}

#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub backend: String,
    pub steps: Vec<InstallStep>,
    pub verify_hint: String,
}

pub fn no_install_mode() -> bool {
    std::env::var("MODELDESK_NO_INSTALL")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn plan(backend: &str, env: &Env) -> Result<InstallPlan> {
    if !env.supports(backend) {
        anyhow::bail!(
            "{backend} is not supported on {} (MLX backends need macOS 14+ on Apple Silicon)",
            env.label()
        );
    }
    let steps = match backend {
        "mlx-dspark" => mlx_dspark_steps(env)?,
        "mlx-lm" => mlx_lm_steps(env)?,
        "llama-cpp" => llama_steps(env)?,
        "ollama" => ollama_steps(env)?,
        _ => anyhow::bail!("unknown backend '{backend}'"),
    };
    Ok(InstallPlan {
        backend: backend.to_string(),
        steps,
        verify_hint: format!("modelsdesk providers"),
    })
}

fn mlx_dspark_steps(env: &Env) -> Result<Vec<InstallStep>> {
    // pipx preferred (isolated venv); uv tool as fallback.
    if env.has(PackageManager::Pipx) {
        return Ok(vec![InstallStep {
            manager: PackageManager::Pipx,
            manager_label: "pipx install",
            args: vec!["mlx-dspark".to_string()],
        }]);
    }
    if env.has(PackageManager::Uv) {
        return Ok(vec![InstallStep {
            manager: PackageManager::Uv,
            manager_label: "uv tool install",
            args: vec!["mlx-dspark".to_string()],
        }]);
    }
    anyhow::bail!("need pipx or uv to install mlx-dspark (brew install pipx)")
}

fn mlx_lm_steps(env: &Env) -> Result<Vec<InstallStep>> {
    if env.has(PackageManager::Brew) {
        return Ok(vec![InstallStep {
            manager: PackageManager::Brew,
            manager_label: "brew install",
            args: vec!["mlx-lm".to_string()],
        }]);
    }
    if env.has(PackageManager::Pipx) {
        return Ok(vec![InstallStep {
            manager: PackageManager::Pipx,
            manager_label: "pipx install",
            args: vec!["mlx-lm".to_string()],
        }]);
    }
    anyhow::bail!("need Homebrew or pipx to install mlx-lm")
}

fn llama_steps(env: &Env) -> Result<Vec<InstallStep>> {
    if env.has(PackageManager::Brew) {
        return Ok(vec![InstallStep {
            manager: PackageManager::Brew,
            manager_label: "brew install",
            args: vec!["llama.cpp".to_string()],
        }]);
    }
    if env.os == Os::Linux {
        let pkg = match env.distro {
            Some(LinuxDistro::Debian) => {
                if !env.has(PackageManager::Apt) {
                    anyhow::bail!("apt-get not found — install llama.cpp manually (https://github.com/ggml-org/llama.cpp)");
                }
                return Ok(vec![InstallStep {
                    manager: PackageManager::Apt,
                    manager_label: "sudo apt-get install -y",
                    args: vec!["llama.cpp".to_string()],
                }]);
            }
            Some(LinuxDistro::Fedora) => {
                if !env.has(PackageManager::Dnf) {
                    anyhow::bail!("dnf not found — install llama.cpp manually");
                }
                return Ok(vec![InstallStep {
                    manager: PackageManager::Dnf,
                    manager_label: "sudo dnf install -y",
                    args: vec!["llama.cpp".to_string()],
                }]);
            }
            Some(LinuxDistro::Arch) => {
                if !env.has(PackageManager::Pacman) {
                    anyhow::bail!("pacman not found — install llama.cpp manually");
                }
                return Ok(vec![InstallStep {
                    manager: PackageManager::Pacman,
                    manager_label: "sudo pacman -S --noconfirm",
                    args: vec!["llama.cpp".to_string()],
                }]);
            }
            _ => "llama.cpp",
        };
        let _ = pkg;
        anyhow::bail!("no supported package manager for llama.cpp on this distro — build from https://github.com/ggml-org/llama.cpp");
    }
    if env.os == Os::Windows {
        if env.has(PackageManager::Winget) {
            return Ok(vec![InstallStep {
                manager: PackageManager::Winget,
                manager_label: "winget install",
                args: vec!["llama.cpp".to_string()],
            }]);
        }
        if env.has(PackageManager::Choco) {
            return Ok(vec![InstallStep {
                manager: PackageManager::Choco,
                manager_label: "choco install -y",
                args: vec!["llama.cpp".to_string()],
            }]);
        }
        anyhow::bail!("need winget or choco to install llama.cpp on Windows");
    }
    anyhow::bail!("no install path for llama.cpp on {}", env.label())
}

fn ollama_steps(env: &Env) -> Result<Vec<InstallStep>> {
    if env.has(PackageManager::Brew) {
        return Ok(vec![InstallStep {
            manager: PackageManager::Brew,
            manager_label: "brew install",
            args: vec!["ollama".to_string()],
        }]);
    }
    if env.os == Os::Linux {
        // Official install script (needs curl) — single step.
        return Ok(vec![InstallStep {
            manager: PackageManager::Apt,
            manager_label: "curl -fsSL https://ollama.com/install.sh | sh",
            args: vec![],
        }]);
    }
    if env.os == Os::Windows {
        if env.has(PackageManager::Winget) {
            return Ok(vec![InstallStep {
                manager: PackageManager::Winget,
                manager_label: "winget install",
                args: vec!["Ollama.Ollama".to_string()],
            }]);
        }
        anyhow::bail!("install Ollama from https://ollama.com/download (or winget)");
    }
    anyhow::bail!("no install path for ollama on {}", env.label())
}

pub fn run_plan(plan: &InstallPlan) -> Result<()> {
    use std::io::Write;
    let tty = console::Term::stdout().is_term();
    for step in &plan.steps {
        let cue = match step.manager {
            PackageManager::Apt | PackageManager::Dnf | PackageManager::Pacman => {
                " (may prompt for sudo)".to_string()
            }
            _ => String::new(),
        };
        if tty {
            println!("{} {}{}", output::cyan("●"), step.describe(), cue);
        } else {
            println!("install: {}{}", step.describe(), cue);
        }
        std::io::stdout().flush().ok();
        let (prog, args) = step_command(step);
        let status = std::process::Command::new(&prog)
            .args(&args)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .with_context(|| format!("running {}", step.describe()))?;
        if !status.success() {
            anyhow::bail!("install step failed: {} ({status})", step.describe());
        }
    }
    println!("{}", output::ok(&format!("{} installed", plan.backend)));
    Ok(())
}

fn step_command(step: &InstallStep) -> (String, Vec<String>) {
    // The ollama-linux "step" is a shell pipeline, not a binary.
    if step.manager_label.starts_with("curl ") {
        return (
            "sh".to_string(),
            vec!["-c".to_string(), step.manager_label.to_string()],
        );
    }
    let prog = match step.manager {
        PackageManager::Brew => "brew",
        PackageManager::Pipx => "pipx",
        PackageManager::Uv => "uv",
        PackageManager::Apt => {
            return (
                "sh".to_string(),
                vec![
                    "-c".to_string(),
                    format!("sudo apt-get install -y {}", step.args.join(" ")),
                ],
            );
        }
        PackageManager::Dnf => {
            return (
                "sh".to_string(),
                vec![
                    "-c".to_string(),
                    format!("sudo dnf install -y {}", step.args.join(" ")),
                ],
            );
        }
        PackageManager::Pacman => {
            return (
                "sh".to_string(),
                vec![
                    "-c".to_string(),
                    format!("sudo pacman -S --noconfirm {}", step.args.join(" ")),
                ],
            );
        }
        PackageManager::Zypper => "zypper",
        PackageManager::Apk => "apk",
        PackageManager::Winget => "winget",
        PackageManager::Choco => "choco",
        PackageManager::Scoop => "scoop",
        PackageManager::Cargo => "cargo",
    };
    let mut args = match step.manager {
        PackageManager::Brew => vec!["install".to_string()],
        PackageManager::Pipx => vec!["install".to_string()],
        PackageManager::Uv => vec!["tool".to_string(), "install".to_string()],
        PackageManager::Winget => vec!["install".to_string()],
        PackageManager::Choco => vec!["install".to_string(), "-y".to_string()],
        PackageManager::Scoop => vec!["install".to_string()],
        PackageManager::Cargo => vec!["install".to_string()],
        _ => vec![],
    };
    args.extend(step.args.clone());
    (prog.to_string(), args)
}

pub fn ensure_arch_ok(backend: &str, env: &Env) -> Result<()> {
    if !env.supports(backend) {
        anyhow::bail!(
            "{backend} is not supported on {} ({})",
            env.label(),
            match backend {
                "mlx-dspark" | "mlx-lm" => "needs macOS 14+ on Apple Silicon",
                _ => "unsupported platform",
            }
        );
    }
    if env.arch == Arch::Other {
        anyhow::bail!("unsupported CPU architecture for {backend}");
    }
    Ok(())
}

pub fn gguf_cache_path(repo: &str, file: Option<&str>) -> PathBuf {
    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let _ = username;
    let slug = repo.replace('/', "--");
    let base = crate::platform::home()
        .join(".cache")
        .join("modelsdesk")
        .join("gguf")
        .join(slug);
    match file {
        Some(f) => base.join(f.rsplit('/').next().unwrap_or(f)),
        None => base,
    }
}
