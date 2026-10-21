use anyhow::Result;
use std::path::PathBuf;

use crate::config::ModelEntry;
use crate::health::HealthReport;
use crate::providers::Provider;

/// TabbyAPI: the official API server for ExllamaV3
/// (https://github.com/theroyallab/tabbyAPI). OpenAI-compatible /v1 routes,
/// draft-model speculative decoding, continuous batching. Linux/Windows with
/// CUDA/ROCm. Runs as `python -m tabbyAPI` (config.yml-driven) or via
/// `start.sh`; the binary probe accepts either entrypoint.
pub struct Tabby;

#[async_trait::async_trait]
impl Provider for Tabby {
    fn id(&self) -> &'static str {
        "tabby"
    }

    fn display(&self) -> &'static str {
        "TabbyAPI (ExllamaV3 backend, OpenAI-compatible server)"
    }

    fn binary(&self) -> Result<PathBuf> {
        // TabbyAPI ships start.sh / start.bat launchers; fall back to python.
        let path = std::env::var_os("PATH").unwrap_or_default();
        for name in ["tabbyAPI", "start-tabby"] {
            for dir in std::env::split_paths(&path) {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
        // python module form: python3 -m tabbyAPI
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("python3");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(anyhow::anyhow!(
            "tabbyAPI not found (git clone https://github.com/theroyallab/tabbyAPI, then ./start.sh — needs CUDA/ROCm)"
        ))
    }

    fn start_args(&self, entry: &ModelEntry, host: &str, port: u16) -> Result<Vec<String>> {
        // TabbyAPI is config.yml-driven; core overrides via CLI:
        // --model-dir + model name, --host/--port, draft model for
        // speculative decoding.
        let model_dir = entry
            .repo
            .rsplit('/')
            .next()
            .unwrap_or(&entry.repo)
            .to_string();
        let mut args = vec![
            "-m".to_string(),
            "tabbyAPI".to_string(),
            "--host".to_string(),
            host.to_string(),
            "--port".to_string(),
            port.to_string(),
            "--model-dir".to_string(),
            model_dir,
        ];
        if let Some(draft) = &entry.drafter {
            args.push("--draft-model-dir".to_string());
            args.push(draft.rsplit('/').next().unwrap_or(draft).to_string());
        }
        if let Some(ctx) = entry.context_window {
            args.push("--max-seq-len".to_string());
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
        // TabbyAPI exposes /v1/models like other OpenAI-compatible servers.
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
            mode: Some("tabby".to_string()),
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
