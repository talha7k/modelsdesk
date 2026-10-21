use anyhow::Result;
use std::path::PathBuf;

use crate::config::ModelEntry;
use crate::health::HealthReport;
use crate::providers::Provider;

/// vLLM: high-throughput Python engine (https://github.com/vllm-project/vllm).
/// `vllm serve <model>` exposes OpenAI-compatible /v1 endpoints (default port 8000).
pub struct Vllm;

#[async_trait::async_trait]
impl Provider for Vllm {
    fn id(&self) -> &'static str {
        "vllm"
    }

    fn display(&self) -> &'static str {
        "vLLM (high-throughput server, OpenAI-compatible API)"
    }

    fn binary(&self) -> Result<PathBuf> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("vllm");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(anyhow::anyhow!("vllm not on PATH"))
    }

    fn start_args(&self, entry: &ModelEntry, host: &str, port: u16) -> Result<Vec<String>> {
        let mut args = vec![
            "serve".to_string(),
            entry.repo.clone(),
            "--host".to_string(),
            host.to_string(),
            "--port".to_string(),
            port.to_string(),
        ];
        if let Some(ctx) = entry.context_window {
            args.push("--max-model-len".to_string());
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
            mode: Some("vllm".to_string()),
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
