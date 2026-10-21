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

    fn min_version(&self) -> Option<(u64, u64, u64)> {
        // b5000+ ships the OpenAI-compatible /health + /v1 routes modelsdesk uses.
        Some((5000, 0, 0))
    }

    async fn fetch_weights(&self, entry: &ModelEntry) -> Result<Option<PathBuf>> {
        use crate::runtime_install::gguf_cache_path;
        let repo = entry.repo.clone();
        // Local file: verify + use directly.
        for candidate in [&repo, &shellexpand(&repo)] {
            let p = PathBuf::from(candidate);
            if p.is_file() {
                return Ok(Some(p));
            }
        }
        // HF repo: resolve a GGUF file and download it into the cache.
        let files = crate::hub::repo_files(&repo).await?;
        let ggufs: Vec<&String> = files
            .iter()
            .filter(|f| f.to_lowercase().ends_with(".gguf"))
            .collect();
        if ggufs.is_empty() {
            anyhow::bail!("repo '{repo}' has no .gguf files for llama.cpp");
        }
        let picked = pick_gguf(&ggufs).to_string();
        let dest = gguf_cache_path(&repo, Some(&picked));
        if dest.is_file() {
            return Ok(Some(dest));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        download_hf(&repo, &picked, &dest).await?;
        Ok(Some(dest))
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
        // Speculative decoding (EAGLE-style): a draft model on the profile
        // enables llama-server's -md path. Local .gguf paths pass through
        // directly; HF repos are resolved+downloaded in fetch_weights.
        if let Some(draft) = &entry.drafter {
            let draft_path = shellexpand(draft);
            if PathBuf::from(&draft_path).is_file() {
                args.push("--model-draft".to_string());
                args.push(draft_path);
                args.push("--draft-max".to_string());
                args.push("16".to_string());
            } else {
                anyhow::bail!(
                    "draft model '{draft}' is not a local .gguf path — download it first (llama-server needs a file for --model-draft)"
                );
            }
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

fn shellexpand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    p.to_string()
}

/// Prefer balanced quants: Q4_K_M > Q4_0 > Q5_K_M > Q8_0 > first available.
fn pick_gguf<'a>(files: &[&'a String]) -> &'a str {
    for prefer in ["q4_k_m", "q4_0", "q5_k_m", "q8_0"] {
        if let Some(f) = files.iter().find(|f| f.to_lowercase().contains(prefer)) {
            return f;
        }
    }
    files[0]
}

async fn download_hf(repo: &str, file: &str, dest: &PathBuf) -> Result<()> {
    use indicatif::{ProgressBar, ProgressStyle};
    let url = format!("https://huggingface.co/{repo}/resolve/main/{file}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3600))
        .build()?;
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("downloading {repo}/{file}: HTTP {}", resp.status());
    }
    let total = resp.content_length().unwrap_or(0);
    let tty = console::Term::stdout().is_term();
    let pb = ProgressBar::new(total);
    if tty {
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}",
            )?,
        );
        pb.set_message(file.to_string());
    } else {
        pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        println!("downloading {repo}/{file} ({} MB)…", total / 1_048_576);
    }
    let mut out = tokio::fs::File::create(dest).await?;
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        out.write_all(&chunk).await?;
        pb.inc(chunk.len() as u64);
    }
    out.flush().await?;
    pb.finish_and_clear();
    if !tty {
        println!("cached {}", dest.display());
    }
    Ok(())
}
