use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::config::ModelEntry;
use crate::health::HealthReport;
use crate::providers::Provider;

pub struct MlxDspark;

fn candidate_bins() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    vec![
        home.join(".local").join("bin").join("mlx-dspark"),
        PathBuf::from("/opt/homebrew/bin/mlx-dspark"),
        PathBuf::from("/usr/local/bin/mlx-dspark"),
    ]
}

#[async_trait::async_trait]
impl Provider for MlxDspark {
    fn id(&self) -> &'static str {
        "mlx-dspark"
    }

    fn display(&self) -> &'static str {
        "mlx-dspark (DSpark speculative decoding, Apple Silicon)"
    }

    fn binary(&self) -> Result<PathBuf> {
        candidate_bins()
            .into_iter()
            .find(|p| p.exists())
            .context("mlx-dspark not found (pipx install mlx-dspark)")
    }

    fn min_version(&self) -> Option<(u64, u64, u64)> {
        Some((0, 17, 0))
    }

    fn start_args(&self, entry: &ModelEntry, host: &str, port: u16) -> Result<Vec<String>> {
        let mut args = vec![
            "serve".to_string(),
            "--model".to_string(),
            entry.repo.clone(),
            "--mode".to_string(),
            entry.mode.clone().unwrap_or_else(|| "dspark".to_string()),
            "--drafter-bits".to_string(),
            "4".to_string(),
            "--kv-bits".to_string(),
            entry.kv_bits.to_string(),
            "--host".to_string(),
            host.to_string(),
            "--port".to_string(),
            port.to_string(),
        ];
        if let Some(drafter) = &entry.drafter {
            args.push("--drafter".to_string());
            args.push(drafter.clone());
        }
        if let Some(ctx) = entry.context_window {
            args.push("--context-window".to_string());
            args.push(ctx.to_string());
        }
        if let Some(temp) = entry.default_temperature {
            args.push("--default-temperature".to_string());
            args.push(temp.to_string());
        }
        Ok(args)
    }

    async fn health(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
    ) -> Result<HealthReport> {
        let url = format!("http://{host}:{port}/health");
        let resp = client.get(&url).send().await?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let ok = status.is_success() && body.get("status").and_then(|v| v.as_str()) == Some("ok");
        Ok(HealthReport {
            ok,
            status_line: format!("HTTP {}", status.as_u16()),
            model: body
                .get("model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            mode: body
                .get("mode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            detail: body.to_string(),
        })
    }

    async fn admin_models(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
    ) -> Result<Option<serde_json::Value>> {
        let url = format!("http://{host}:{port}/admin/models");
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => Ok(resp.json().await.ok()),
            _ => Ok(None),
        }
    }

    async fn admin_load(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
        entry: &ModelEntry,
    ) -> Result<bool> {
        let url = format!("http://{host}:{port}/admin/load");
        let mut payload = serde_json::json!({ "model": entry.repo });
        if let Some(drafter) = &entry.drafter {
            payload["drafter"] = serde_json::json!(drafter);
        }
        let resp = client.post(&url).json(&payload).send().await?;
        Ok(resp.status().is_success())
    }
}
