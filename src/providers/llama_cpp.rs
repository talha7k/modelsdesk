use anyhow::Result;
use std::path::PathBuf;

use crate::config::ModelEntry;
use crate::health::HealthReport;
use crate::providers::Provider;

pub struct LlamaCpp;

#[async_trait::async_trait]
impl Provider for LlamaCpp {
    fn id(&self) -> &'static str {
        "llama-cpp"
    }

    fn display(&self) -> &'static str {
        "llama.cpp server (GGUF, cross-platform)"
    }

    fn binary(&self) -> Result<PathBuf> {
        for name in ["llama-server", "llama_server", "server"] {
            if let Ok(path) = which(name) {
                return Ok(path);
            }
        }
        anyhow::bail!("llama-server not found (brew install llama.cpp)")
    }

    fn start_args(&self, entry: &ModelEntry, host: &str, port: u16) -> Result<Vec<String>> {
        let mut args = vec![
            "--model".to_string(),
            entry.repo.clone(),
            "--host".to_string(),
            host.to_string(),
            "--port".to_string(),
            port.to_string(),
        ];
        if let Some(ctx) = entry.context_window {
            args.push("--ctx-size".to_string());
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
        let url = format!("http://{host}:{port}/health");
        let resp = client.get(&url).send().await?;
        let status = resp.status();
        let ok = status.is_success();
        Ok(HealthReport {
            ok,
            status_line: format!("HTTP {}", status.as_u16()),
            model: None,
            mode: Some("llama.cpp".to_string()),
            detail: String::new(),
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

fn which(name: &str) -> Result<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(anyhow::anyhow!("{name} not on PATH"))
}
