use anyhow::Result;
use std::path::PathBuf;

use crate::config::ModelEntry;
use crate::health::HealthReport;
use crate::providers::Provider;

/// SGLang: fast serving engine (https://github.com/sgl-project/sglang).
/// `python -m sglang.launch_server --model-path <model>` exposes OpenAI-compatible
/// /v1 endpoints (default port 30000).
pub struct Sglang;

#[async_trait::async_trait]
impl Provider for Sglang {
    fn id(&self) -> &'static str {
        "sglang"
    }

    fn display(&self) -> &'static str {
        "SGLang (fast serving engine, OpenAI-compatible API)"
    }

    fn binary(&self) -> Result<PathBuf> {
        // sglang runs as a python module; locate python3 and check importability lazily.
        let path = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("python3");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(anyhow::anyhow!("python3 not on PATH (sglang is a python module)"))
    }

    fn start_args(&self, entry: &ModelEntry, host: &str, port: u16) -> Result<Vec<String>> {
        let mut args = vec![
            "-m".to_string(),
            "sglang.launch_server".to_string(),
            "--model-path".to_string(),
            entry.repo.clone(),
            "--host".to_string(),
            host.to_string(),
            "--port".to_string(),
            port.to_string(),
        ];
        if let Some(ctx) = entry.context_window {
            args.push("--context-length".to_string());
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
            mode: Some("sglang".to_string()),
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
