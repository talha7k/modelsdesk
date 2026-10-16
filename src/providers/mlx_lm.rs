use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::config::ModelEntry;
use crate::health::HealthReport;
use crate::providers::Provider;

fn candidate_bins() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin/mlx_lm.server"),
        PathBuf::from("/usr/local/bin/mlx_lm.server"),
    ]
}

pub struct MlxLm;

#[async_trait::async_trait]
impl Provider for MlxLm {
    fn id(&self) -> &'static str {
        "mlx-lm"
    }

    fn display(&self) -> &'static str {
        "mlx-lm server (plain MLX, Apple Silicon)"
    }

    fn binary(&self) -> Result<PathBuf> {
        candidate_bins()
            .into_iter()
            .find(|p| p.exists())
            .context("mlx_lm.server not found (brew install mlx-lm)")
    }

    fn start_args(&self, entry: &ModelEntry, host: &str, port: u16) -> Result<Vec<String>> {
        let mut args = vec![
            "--model".to_string(),
            entry.repo.clone(),
            "--host".to_string(),
            host.to_string(),
            "--port".to_string(),
            port.to_string(),
            "--trust-remote-code".to_string(),
        ];
        if entry.kv_bits == 4 || entry.kv_bits == 8 {
            args.push("--kv-bits".to_string());
            args.push(entry.kv_bits.to_string());
        }
        Ok(args)
    }

    async fn health(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
    ) -> Result<HealthReport> {
        let url = format!("http://{host}:{port}/v1/models");
        let resp = client.get(&url).send().await?;
        let ok = resp.status().is_success();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let model = body
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|a| a.first())
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(HealthReport {
            ok,
            status_line: "HTTP 200".to_string(),
            model,
            mode: Some("baseline".to_string()),
            detail: body.to_string(),
        })
    }

    async fn admin_models(
        &self,
        _client: &reqwest::Client,
        _host: &str,
        _port: u16,
    ) -> Result<Option<serde_json::Value>> {
        Ok(None)
    }

    async fn admin_load(
        &self,
        _client: &reqwest::Client,
        _host: &str,
        _port: u16,
        _entry: &ModelEntry,
    ) -> Result<bool> {
        Ok(false)
    }
}
