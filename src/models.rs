use anyhow::Result;
use serde::Serialize;

use crate::config::Config;
use crate::platform;
use crate::providers;

#[derive(Debug, Serialize)]
pub struct LocalModel {
    pub source: String,
    pub repo: String,
    pub size_bytes: Option<u64>,
    pub active: bool,
}

#[derive(Debug, Serialize)]
pub struct RegistryModel {
    pub id: String,
    pub target: String,
    pub mode: String,
    pub ram: String,
    pub fits: bool,
    pub ready: bool,
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let ft = entry.file_type();
            let is_dir = ft.map(|t| t.is_dir()).unwrap_or(false)
                || std::fs::metadata(&p).map(|m| m.is_dir()).unwrap_or(false);
            if is_dir {
                stack.push(p);
            } else if let Ok(meta) = std::fs::metadata(&p) {
                total += meta.len();
            }
        }
    }
    total
}

pub fn local_models(cfg: &Config) -> Vec<LocalModel> {
    let mut out = Vec::new();
    let hub = platform::hf_hub_cache();
    if let Ok(entries) = std::fs::read_dir(&hub) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(repo) = name.strip_prefix("models--").map(|s| s.replace("--", "/")) else {
                continue;
            };
            let snapshots = entry.path().join("snapshots");
            let has_weights = std::fs::read_dir(&snapshots)
                .map(|d| {
                    d.flatten().any(|e| {
                        let p = e.path();
                        let name = p
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if name.ends_with(".safetensors") {
                            return true;
                        }
                        if e.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                            if let Ok(target) = std::fs::read_link(&p) {
                                let resolved = if target.is_absolute() {
                                    target
                                } else {
                                    p.parent().unwrap_or(&p).join(target)
                                };
                                if resolved
                                    .file_name()
                                    .is_some_and(|n| n.to_string_lossy().ends_with(".safetensors"))
                                {
                                    return true;
                                }
                            }
                        }
                        let resolved = if e.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                            std::fs::canonicalize(&p).unwrap_or(p.clone())
                        } else {
                            p.clone()
                        };
                        if resolved.is_dir() {
                            return std::fs::read_dir(&resolved)
                                .map(|mut d| {
                                    d.any(|f| {
                                        f.map(|f| {
                                            f.file_name()
                                                .to_string_lossy()
                                                .ends_with(".safetensors")
                                        })
                                        .unwrap_or(false)
                                    })
                                })
                                .unwrap_or(false);
                        }
                        false
                    })
                })
                .unwrap_or(false);
            if !has_weights {
                continue;
            }
            let active = cfg.active.as_ref().is_some_and(|a| a == &repo)
                || cfg
                    .models
                    .iter()
                    .any(|m| m.repo == repo && Some(&m.name) == cfg.active.as_ref());
            out.push(LocalModel {
                source: "huggingface".to_string(),
                repo,
                size_bytes: Some(dir_size(&entry.path())),
                active,
            });
        }
    }
    let lmstudio = platform::lmstudio_models();
    if lmstudio.exists() {
        let mut stack = vec![lmstudio];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let has_config = p.join("config.json").exists();
                    let has_weights = p
                        .read_dir()
                        .map(|mut d| {
                            d.any(|e| {
                                e.map(|e| e.file_name().to_string_lossy().ends_with(".safetensors"))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false);
                    if has_config && has_weights {
                        out.push(LocalModel {
                            source: "lmstudio".to_string(),
                            repo: p.to_string_lossy().to_string(),
                            size_bytes: Some(dir_size(&p)),
                            active: false,
                        });
                    } else {
                        stack.push(p);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.repo.cmp(&b.repo));
    out
}

pub async fn registry_models(cfg: &Config) -> Result<Vec<RegistryModel>> {
    let client = crate::server::http_client();
    let entry = cfg
        .active_entry()
        .unwrap_or_else(|_| crate::config::ModelEntry {
            name: String::new(),
            provider: "mlx-dspark".to_string(),
            repo: String::new(),
            drafter: None,
            kv_bits: 4,
            context_window: None,
            default_temperature: None,
            mode: None,
        });
    let provider = providers::for_entry(&entry);
    let Some(value) = provider.admin_models(&client, &cfg.host, cfg.port).await? else {
        return Ok(Vec::new());
    };
    let models = value
        .get("models")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(models
        .iter()
        .map(|m| RegistryModel {
            id: m
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string(),
            target: m
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string(),
            mode: m
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string(),
            ram: m
                .get("ram")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string(),
            fits: m.get("fits").and_then(|v| v.as_bool()).unwrap_or(false),
            ready: m.get("ready").and_then(|v| v.as_bool()).unwrap_or_else(|| {
                m.get("target_installed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            }),
        })
        .collect())
}

pub fn format_bytes(bytes: u64) -> String {
    const GB: f64 = 1_073_741_824.0;
    const MB: f64 = 1_048_576.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", b / MB)
    }
}
