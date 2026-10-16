use anyhow::Result;
use std::path::PathBuf;

use crate::config::ModelEntry;
use crate::health::HealthReport;
use crate::providers::Provider;

pub struct Ollama;

#[async_trait::async_trait]
impl Provider for Ollama {
    fn id(&self) -> &'static str {
        "ollama"
    }

    fn display(&self) -> &'static str {
        "Ollama (managed daemon, cross-platform)"
    }

    fn binary(&self) -> Result<PathBuf> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("ollama");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(anyhow::anyhow!("ollama not on PATH"))
    }

    fn start_args(&self, entry: &ModelEntry, _host: &str, _port: u16) -> Result<Vec<String>> {
        Ok(vec!["run".to_string(), entry.repo.clone()])
    }

    async fn health(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
    ) -> Result<HealthReport> {
        let url = format!("http://{host}:{port}/api/tags");
        let resp = client.get(&url).send().await?;
        let status = resp.status();
        let ok = status.is_success();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        Ok(HealthReport {
            ok,
            status_line: format!("HTTP {}", status.as_u16()),
            model: None,
            mode: Some("ollama".to_string()),
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
