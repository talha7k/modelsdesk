use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::compat::{self, ModelFormat};
use crate::output;

#[derive(Debug, Clone, Deserialize)]
pub struct HfHit {
    pub id: String,
    #[serde(default)]
    pub likes: i64,
    #[serde(default)]
    pub downloads: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub repo: String,
    pub likes: i64,
    pub downloads: Option<i64>,
    pub format: String,
    pub provider: String,
    pub provider_installed: bool,
    pub estimate_gb: f64,
    pub fits: bool,
    pub cached: bool,
}

fn hf_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("modeldesk/0.1.0")
        .build()
        .expect("hf http client")
}

pub async fn search(query: &str, limit: usize) -> Result<Vec<HfHit>> {
    let client = hf_client();
    let resp = client
        .get("https://huggingface.co/api/models")
        .query(&[
            ("search", query),
            ("filter", "text-generation"),
            ("sort", "downloads"),
            ("direction", "-1"),
            ("limit", &limit.min(50).to_string()),
        ])
        .send()
        .await
        .context("contacting huggingface.co (network down?)")?;
    if !resp.status().is_success() {
        anyhow::bail!("huggingface search failed: HTTP {}", resp.status());
    }
    Ok(resp.json().await.context("parsing huggingface response")?)
}

pub fn recommend_provider(format: ModelFormat) -> &'static str {
    match format {
        ModelFormat::MlX => "mlx-dspark",
        ModelFormat::Gguf => "llama-cpp",
        ModelFormat::Safetensors | ModelFormat::Unknown => "mlx-dspark",
    }
}

pub fn enrich(hits: Vec<HfHit>) -> Vec<SearchHit> {
    hits
        .into_iter()
        .map(|h| {
            let format = compat::detect_format(&h.id);
            let provider = recommend_provider(format).to_string();
            let entry = crate::config::ModelEntry {
                name: String::new(),
                provider: provider.clone(),
                repo: h.id.clone(),
                drafter: None,
                kv_bits: 4,
                context_window: None,
                default_temperature: None,
                mode: None,
            };
            let provider_installed = crate::providers::for_entry(&entry).binary().is_ok();
            let estimate_gb = compat::estimate_need_gb(&entry);
            let fits = estimate_gb <= compat::total_memory_gb() * 0.85;
            let cached = cached_repos().iter().any(|r| r == &h.id);
            SearchHit {
                repo: h.id,
                likes: h.likes,
                downloads: h.downloads,
                format: format!("{format:?}"),
                provider,
                provider_installed,
                estimate_gb,
                fits,
                cached,
            }
        })
        .collect()
}

fn cached_repos() -> Vec<String> {
    let hub = crate::platform::hf_hub_cache();
    std::fs::read_dir(&hub)
        .map(|d| {
            d.flatten()
                .filter_map(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .strip_prefix("models--")
                        .map(|s| s.replace("--", "/"))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn print_hits(hits: &[SearchHit], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }
    if hits.is_empty() {
        println!(
            "{}",
            output::dim("no runnable models found — try a broader query")
        );
        return Ok(());
    }
    println!(
        "{}",
        output::bold("huggingface results (runnable formats):")
    );
    for h in hits {
        let dl = h
            .downloads
            .map(|d| {
                if d >= 1000 {
                    format!("{:.0}k dl", d as f64 / 1000.0)
                } else {
                    format!("{d} dl")
                }
            })
            .unwrap_or_else(|| "? dl".to_string());
        let fit = if h.cached {
            output::cyan("cached")
        } else if h.fits {
            output::ok("fits")
        } else {
            output::err("too big")
        };
        let prov = if h.provider_installed {
            output::dim(&h.provider)
        } else {
            output::dim(&format!("{} (not installed)", h.provider))
        };
        println!(
            "  {} {} {} {} ~{:.1} GB {}",
            h.repo,
            output::dim(&format!("♥{} {}", h.likes, dl)),
            output::dim(&format!("[{}]", h.format)),
            prov,
            h.estimate_gb,
            fit
        );
    }
    println!(
        "\n{}",
        output::dim("setup one: `modeldesk setup <repo> [--provider ID] [--name NAME]`")
    );
    Ok(())
}

pub async fn repo_files(repo: &str) -> Result<Vec<String>> {
    let client = hf_client();
    let url = format!("https://huggingface.co/api/models/{repo}");
    let resp = client
        .get(&url)
        .send()
        .await
        .context(format!("fetching repo info for {repo}"))?;
    if resp.status().as_u16() == 404 {
        anyhow::bail!("repo '{repo}' not found on huggingface.co");
    }
    if !resp.status().is_success() {
        anyhow::bail!("huggingface repo lookup failed: HTTP {}", resp.status());
    }
    let v: serde_json::Value = resp.json().await.context("parsing repo info")?;
    Ok(v
        .get("siblings")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|s| {
            s.get("rfilename")
                .and_then(|f| f.as_str())
                .map(|f| f.to_string())
        })
        .collect())
}

pub fn is_cached(repo: &str) -> bool {
    cached_repos().iter().any(|r| r == repo)
}
