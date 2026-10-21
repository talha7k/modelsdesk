use anyhow::Result;
use std::path::PathBuf;

use crate::config::ModelEntry;
use crate::health::HealthReport;
use crate::providers::Provider;

/// Strata: Rust-based MoE/GGUF engine with an OpenAI-compatible server
/// (https://github.com/Niko1221/Strata). Terminal `strata launch` serves
/// /v1/chat/completions and /v1/models on the given port.
pub struct Strata;

#[async_trait::async_trait]
impl Provider for Strata {
    fn id(&self) -> &'static str {
        "strata"
    }

    fn display(&self) -> &'static str {
        "Strata (Rust engine, MoE GGUF, OpenAI-compatible server)"
    }

    fn binary(&self) -> Result<PathBuf> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("strata");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(anyhow::anyhow!("strata not on PATH"))
    }

    fn match_score(&self, entry: &ModelEntry) -> u8 {
        let base = if entry.provider == self.id() { 100 } else { 0 };
        // Strata specializes in MoE GGUF weights; nudge it above the generic
        // default when the repo looks like a MoE model.
        if base == 0 {
            let lower = entry.repo.to_lowercase();
            if lower.contains("gguf")
                && (lower.contains("moe") || lower.contains("mixtral") || lower.contains("qwen"))
            {
                return 10;
            }
        }
        base
    }

    fn start_args(&self, entry: &ModelEntry, host: &str, port: u16) -> Result<Vec<String>> {
        let mut args = vec![
            "launch".to_string(),
            "--model".to_string(),
            entry.repo.clone(),
            "--host".to_string(),
            host.to_string(),
            "--port".to_string(),
            port.to_string(),
            "--yes".to_string(),
            "--no-tui".to_string(),
        ];
        if let Some(ctx) = entry.context_window {
            args.push("--context".to_string());
            args.push(ctx.to_string());
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
        let status = resp.status();
        let ok = status.is_success();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let model = body
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|m| m.get("id"))
            .and_then(|i| i.as_str())
            .map(|s| s.to_string());
        Ok(HealthReport {
            ok,
            status_line: format!("HTTP {}", status.as_u16()),
            model,
            mode: Some("strata".to_string()),
            detail: body.to_string(),
        })
    }

    async fn admin_models(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
    ) -> Result<Option<serde_json::Value>> {
        let url = format!("http://{host}:{port}/v1/models");
        let resp = client.get(&url).send().await?;
        Ok(Some(resp.json().await.unwrap_or_default()))
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
