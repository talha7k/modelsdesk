use anyhow::Result;
use async_trait::async_trait;

use crate::config::ModelEntry;
use crate::health::HealthReport;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    Running { pid: u32, port: u16 },
    Stopped,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn display(&self) -> &'static str;
    fn start_args(&self, entry: &ModelEntry, host: &str, port: u16) -> Result<Vec<String>>;
    fn binary(&self) -> Result<std::path::PathBuf>;
    fn match_score(&self, entry: &ModelEntry) -> u8 {
        if entry.provider == self.id() {
            100
        } else {
            0
        }
    }
    async fn health(&self, client: &reqwest::Client, host: &str, port: u16)
        -> Result<HealthReport>;
    async fn admin_models(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
    ) -> Result<Option<serde_json::Value>>;
    async fn admin_load(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
        entry: &ModelEntry,
    ) -> Result<bool>;
}

pub mod llama_cpp;
pub mod mlx_dspark;
pub mod mlx_lm;
pub mod ollama;

pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(mlx_dspark::MlxDspark),
        Box::new(mlx_lm::MlxLm),
        Box::new(llama_cpp::LlamaCpp),
        Box::new(ollama::Ollama),
    ]
}

pub fn for_entry(entry: &ModelEntry) -> Box<dyn Provider> {
    let mut ranked: Vec<(u8, Box<dyn Provider>)> = all()
        .into_iter()
        .map(|p| {
            let s = p.match_score(entry);
            (s, p)
        })
        .collect();
    ranked.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    ranked.into_iter().next().map(|(_, p)| p).unwrap()
}
