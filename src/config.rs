use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

use crate::platform;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub name: String,
    pub provider: String,
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drafter: Option<String>,
    #[serde(default = "default_kv_bits", skip_serializing_if = "is_default_kv")]
    pub kv_bits: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

fn default_kv_bits() -> u8 {
    4
}

fn is_default_kv(v: &u8) -> bool {
    *v == 4
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8090
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            active: Some("lfm-2.5-8b".to_string()),
            models: vec![ModelEntry {
                name: "lfm-2.5-8b".to_string(),
                provider: "mlx-dspark".to_string(),
                repo: "LiquidAI/LFM2.5-8B-A1B-MLX-4bit".to_string(),
                drafter: Some("LiquidAI/LFM2.5-8B-A1B-DSpark".to_string()),
                kv_bits: 4,
                context_window: None,
                default_temperature: None,
                mode: None,
            }],
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = platform::config_path();
        if !path.exists() {
            let cfg = Config::default();
            cfg.save()?;
            return Ok(cfg);
        }
        let raw =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = platform::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        fs::write(&path, raw)?;
        Ok(())
    }

    pub fn active_entry(&self) -> Result<ModelEntry> {
        let want = self.active.as_deref().unwrap_or("");
        self.models
            .iter()
            .find(|m| m.name == want || m.repo == want)
            .cloned()
            .with_context(|| {
                format!(
                    "no model named '{want}' in {} (see `modelsdesk models`)",
                    platform::config_path().display()
                )
            })
    }

    pub fn resolve(&self, name: &str) -> Option<ModelEntry> {
        self.models
            .iter()
            .find(|m| m.name == name || m.repo == name)
            .cloned()
    }
}
