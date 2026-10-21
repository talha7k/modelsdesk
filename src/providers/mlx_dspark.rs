use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::config::ModelEntry;
use crate::health::HealthReport;
use crate::providers::Provider;

pub struct MlxDspark;

/// Known target → DSpark/DFlash drafter pairs (verified via `mlx-dspark
/// models`). Targets are quantization-agnostic: the repo prefix matches, any
/// quant of the same model gets the same drafter.
pub const KNOWN_DRAFTERS: &[(&str, &str, &str)] = &[
    // (target repo prefix, DSpark drafter, DFlash drafter or "")
    ("mlx-community/Qwen3-4B", "deepseek-ai/dspark_qwen3_4b_block7", "z-lab/Qwen3-4B-DFlash-b16"),
    ("mlx-community/Qwen3-8B", "deepseek-ai/dspark_qwen3_8b_block7", "z-lab/Qwen3-8B-DFlash-b16"),
    ("mlx-community/Qwen3-14B", "deepseek-ai/dspark_qwen3_14b_block7", ""),
    ("mlx-community/gemma-4-12B-it", "deepseek-ai/dspark_gemma4_12b_block7", "z-lab/gemma4-12B-it-DFlash"),
    ("prism-ml/Ternary-Bonsai-27B-mlx", "Rahim/Ternary-Bonsai-27B-dspark", ""),
    ("mlx-community/Qwen3.6-27B", "satgeze/Qwen3.6-27B-DSpark", ""),
    ("mlx-community/Ornith-1.0-9B", "stanleyphoong/Ornith-1.0-9B-DSpark", ""),
    ("mlx-community/Qwen3.6-35B-A3B", "Koopah/Qwen3.6-35B-A3B-NVFP4-DSPARK", ""),
    ("mlx-community/Qwen3.8-27B", "RedHatAI/Qwen3.8-27B-speculator.dspark", "incoai/Qwen3.8-27B-DFlash2"),
    ("mlx-community/NVIDIA-Nemotron-3.5-Lightning-30B-A3B", "mlx-community/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-DSpark-bf16", ""),
    ("mlx-community/Muse-Glimmer-30B", "DaoCloud/Muse-Glimmer-30B-DSpark", ""),
    ("LiquidAI/LFM2.5-2.6B-MLX", "LiquidAI/LFM2.5-2.6B-DSpark", ""),
    ("LiquidAI/LFM2.5-1.2B-Instruct-MLX", "LiquidAI/LFM2.5-1.2B-Instruct-DSpark", ""),
    ("LiquidAI/LFM2.5-8B-A1B-MLX", "LiquidAI/LFM2.5-8B-A1B-DSpark", ""),
    ("openbmb/MiniCPM5-2B-MLX", "openbmb/MiniCPM5-2B-DSpark", ""),
    ("openbmb/MiniCPM-V-4.7-1B", "openbmb/MiniCPM-V-4.7-1B-DSpark", ""),
    ("openbmb/MiniCPM-V-4.7-35B-A3B", "openbmb/MiniCPM-V-4.7-35B-A3B-DSpark", ""),
];

/// Resolve the drafter for a target repo in the requested mode. Returns
/// (drafter, effective_mode): when no DSpark/DFlash head exists, falls back
/// to drafter-free lookup mode so ANY repo still gets some speculation.
pub fn resolve_drafter(repo: &str, mode: &str) -> (Option<String>, String) {
    let hit = KNOWN_DRAFTERS.iter().find(|(t, _, _)| repo.starts_with(t));
    match (mode, hit) {
        ("dflash", Some((_, _, df))) if !df.is_empty() => {
            (Some(df.to_string()), "dflash".to_string())
        }
        ("dflash", _) => (None, "lookup".to_string()), // no DFlash head → lookup
        (_, Some((_, ds, _))) => (Some(ds.to_string()), mode.to_string()),
        _ => (None, "lookup".to_string()), // unknown target → drafter-free
    }
}

fn candidate_bins() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    vec![
        home.join(".local").join("bin").join("mlx-dspark"),
        PathBuf::from("/opt/homebrew/bin/mlx-dspark"),
        PathBuf::from("/usr/local/bin/mlx-dspark"),
    ]
}

#[async_trait::async_trait]
impl Provider for MlxDspark {
    fn id(&self) -> &'static str {
        "mlx-dspark"
    }

    fn display(&self) -> &'static str {
        "mlx-dspark (DSpark speculative decoding, Apple Silicon)"
    }

    fn binary(&self) -> Result<PathBuf> {
        candidate_bins()
            .into_iter()
            .find(|p| p.exists())
            .context("mlx-dspark not found (pipx install mlx-dspark)")
    }

    fn min_version(&self) -> Option<(u64, u64, u64)> {
        Some((0, 17, 0))
    }

    fn start_args(&self, entry: &ModelEntry, host: &str, port: u16) -> Result<Vec<String>> {
        let mode = entry.mode.clone().unwrap_or_else(|| "dspark".to_string());
        // Auto-resolve the drafter when unset; fall back sensibly per mode.
        let (drafter, mode) = match (&entry.drafter, mode.as_str()) {
            (Some(d), m) => (Some(d.clone()), m.to_string()),
            (None, "baseline") => (None, "baseline".to_string()),
            (None, "lookup") => (None, "lookup".to_string()),
            (None, m) => resolve_drafter(&entry.repo, m),
        };
        let mut args = vec![
            "serve".to_string(),
            "--model".to_string(),
            entry.repo.clone(),
            "--mode".to_string(),
            mode.clone(),
            "--drafter-bits".to_string(),
            "4".to_string(),
            "--kv-bits".to_string(),
            entry.kv_bits.to_string(),
            "--host".to_string(),
            host.to_string(),
            "--port".to_string(),
            port.to_string(),
        ];
        if let Some(d) = drafter {
            args.push("--drafter".to_string());
            args.push(d);
        }
        if let Some(ctx) = entry.context_window {
            args.push("--context-window".to_string());
            args.push(ctx.to_string());
        }
        if let Some(temp) = entry.default_temperature {
            args.push("--default-temperature".to_string());
            args.push(temp.to_string());
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
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let ok = status.is_success() && body.get("status").and_then(|v| v.as_str()) == Some("ok");
        Ok(HealthReport {
            ok,
            status_line: format!("HTTP {}", status.as_u16()),
            model: body
                .get("model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            mode: body
                .get("mode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            detail: body.to_string(),
        })
    }

    async fn admin_models(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
    ) -> Result<Option<serde_json::Value>> {
        let url = format!("http://{host}:{port}/admin/models");
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => Ok(resp.json().await.ok()),
            _ => Ok(None),
        }
    }

    async fn admin_load(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
        entry: &ModelEntry,
    ) -> Result<bool> {
        let url = format!("http://{host}:{port}/admin/load");
        let mut payload = serde_json::json!({ "model": entry.repo });
        if let Some(drafter) = &entry.drafter {
            payload["drafter"] = serde_json::json!(drafter);
        }
        let resp = client.post(&url).json(&payload).send().await?;
        Ok(resp.status().is_success())
    }
}
