use sysinfo::{ProcessesToUpdate, System};

use crate::config::ModelEntry;
use crate::error::ModeldeskError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    MlX,
    Gguf,
    Safetensors,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct CompatReport {
    pub format: ModelFormat,
    pub provider_ok: bool,
    pub arch_ok: bool,
    pub memory_ok: bool,
    pub notes: Vec<String>,
}

pub fn detect_format(repo: &str) -> ModelFormat {
    let lower = repo.to_lowercase();
    if lower.ends_with(".gguf") || lower.contains(".gguf") || lower.contains("-gguf") {
        return ModelFormat::Gguf;
    }
    if lower.contains("-mlx") || lower.contains("mlx-") || lower.contains("/mlx-") {
        return ModelFormat::MlX;
    }
    if lower.ends_with(".safetensors") || lower.contains("safetensors") {
        return ModelFormat::Safetensors;
    }
    ModelFormat::Unknown
}

pub fn provider_accepts(provider: &str, format: ModelFormat, repo: &str) -> Result<(), String> {
    match (provider, format) {
        ("mlx-dspark" | "mlx-lm", ModelFormat::MlX) => Ok(()),
        ("mlx-dspark" | "mlx-lm", ModelFormat::Safetensors) => {
            if repo.starts_with("LiquidAI/") || repo.starts_with("mlx-community/") {
                Ok(())
            } else {
                Err("raw safetensors need an MLX conversion for mlx backends".to_string())
            }
        }
        ("llama-cpp" | "ollama", ModelFormat::Gguf) => Ok(()),
        ("ollama", _) => {
            Err("ollama pulls its own registry names — use `ollama pull <name>` first".to_string())
        }
        (p, f) => Err(format!("{p} cannot serve {f:?} weights")),
    }
}

pub fn arch_ok(provider: &str) -> Result<(), String> {
    let mlx_only = matches!(provider, "mlx-dspark" | "mlx-lm");
    if mlx_only && !cfg!(target_arch = "aarch64") {
        return Err("MLX backends need Apple Silicon (arm64)".to_string());
    }
    if mlx_only && !cfg!(target_os = "macos") {
        return Err("MLX backends are macOS-only".to_string());
    }
    Ok(())
}

pub fn usable_memory_gb() -> f64 {
    let mut sys = System::new();
    sys.refresh_memory();
    (sys.available_memory() as f64) / 1_073_741_824.0
}

pub fn total_memory_gb() -> f64 {
    let mut sys = System::new();
    sys.refresh_memory();
    (sys.total_memory() as f64) / 1_073_741_824.0
}

pub fn estimate_need_gb(entry: &ModelEntry) -> f64 {
    let lower = entry.repo.to_lowercase();
    let params_b: f64 = if lower.contains("27b") || lower.contains("30b") || lower.contains("32b") {
        27.0
    } else if lower.contains("14b") || lower.contains("12b") {
        13.0
    } else if lower.contains("9b") || lower.contains("8b") {
        8.0
    } else if lower.contains("4b") {
        4.0
    } else if lower.contains("2.6b") || lower.contains("3b") {
        2.6
    } else if lower.contains("1.2b") || lower.contains("1b") {
        1.2
    } else {
        8.0
    };
    let bytes_per_param =
        if lower.contains("4bit") || lower.contains("4-bit") || lower.contains("q4") {
            0.55
        } else if lower.contains("8bit") || lower.contains("8-bit") || lower.contains("q8") {
            1.05
        } else if lower.contains("bf16") || lower.contains("fp16") || lower.contains("f16") {
            2.1
        } else {
            1.05
        };
    let drafter_gb = entry.drafter.as_ref().map(|_| 1.5).unwrap_or(0.0);
    params_b * bytes_per_param + 1.5 + drafter_gb
}

pub fn port_holder(port: u16) -> Option<String> {
    if let Some(holder) = port_holder_sysinfo(port) {
        return Some(holder);
    }
    port_holder_lsof(port)
}

fn port_holder_sysinfo(port: u16) -> Option<String> {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    for (_, proc_) in sys.processes() {
        let cmd = proc_
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        if cmd.contains(&port.to_string())
            && (cmd.contains("mlx-dspark")
                || cmd.contains("mlx_lm")
                || cmd.contains("llama-server")
                || cmd.contains("ollama"))
        {
            return Some(format!(
                "{} (pid {})",
                proc_.name().to_string_lossy(),
                proc_.pid()
            ));
        }
    }
    None
}

#[cfg(unix)]
fn port_holder_lsof(port: u16) -> Option<String> {
    let out = std::process::Command::new("lsof")
        .args(["-ti", &format!(":{port}")])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let pids = String::from_utf8_lossy(&out.stdout);
    let mut names = Vec::new();
    for pid in pids.split_whitespace().take(3) {
        let name = std::process::Command::new("ps")
            .args(["-o", "comm=", "-p", pid])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "?".to_string());
        names.push(format!("{name} (pid {pid})"));
    }
    if names.is_empty() {
        None
    } else {
        Some(names.join(", "))
    }
}

#[cfg(windows)]
fn port_holder_lsof(_port: u16) -> Option<String> {
    None
}

pub fn check(entry: &ModelEntry, port: u16) -> Result<CompatReport, ModeldeskError> {
    let format = detect_format(&entry.repo);
    let mut notes = Vec::new();

    provider_accepts(&entry.provider, format, &entry.repo).map_err(|reason| {
        ModeldeskError::Incompatible {
            repo: entry.repo.clone(),
            provider: entry.provider.clone(),
            reason,
        }
    })?;

    arch_ok(&entry.provider).map_err(|reason| ModeldeskError::Incompatible {
        repo: entry.repo.clone(),
        provider: entry.provider.clone(),
        reason,
    })?;

    let need_gb = estimate_need_gb(entry);
    let have_gb = total_memory_gb();
    if need_gb > have_gb * 0.85 {
        return Err(ModeldeskError::InsufficientMemory {
            repo: entry.repo.clone(),
            need_gb,
            have_gb,
        });
    }
    if need_gb > have_gb * 0.6 {
        notes.push(format!(
            "tight fit: ~{need_gb:.1} GB on a {have_gb:.0} GB machine — close other apps"
        ));
    }

    if format == ModelFormat::Unknown {
        notes.push("format not detected from repo name — assuming MLX/Safetensors".to_string());
    }

    if let Some(holder) = port_holder(port) {
        notes.push(format!("port :{port} busy ({holder})"));
    }

    Ok(CompatReport {
        format,
        provider_ok: true,
        arch_ok: true,
        memory_ok: true,
        notes,
    })
}
