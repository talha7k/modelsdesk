use anyhow::{Context, Result};

use crate::config::{Config, ModelEntry};
use crate::output;

fn opencode_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".config")
        .join("opencode")
        .join("opencode.json")
}

fn jcode_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".jcode")
        .join("config.toml")
}

fn served_id(entry: &ModelEntry) -> String {
    entry
        .repo
        .split('/')
        .next_back()
        .unwrap_or(&entry.repo)
        .to_string()
}

fn context_for(entry: &ModelEntry) -> i64 {
    entry.context_window.map(|c| c as i64).unwrap_or(32768)
}

/// Sync agent configs to the active profile.
/// `quiet=true` (auto-hook): no stdout on success, stderr-only partial notes.
/// `quiet=false` (explicit command): human report, or JSON when `json=true`.
pub fn sync(cfg: &Config, quiet: bool, json: bool) -> Result<()> {
    let entry = cfg.active_entry()?;
    let model_id = served_id(&entry);
    let base_url = format!("http://{}:{}/v1", cfg.host, cfg.port);
    let mut synced = Vec::new();
    let mut skipped = Vec::new();

    match sync_opencode(&model_id, &base_url, context_for(&entry)) {
        Ok(path) => synced.push(format!("opencode ({})", path.display())),
        Err(e) => skipped.push(format!("opencode: {e:#}")),
    }
    match sync_jcode(&model_id, &base_url) {
        Ok(path) => synced.push(format!("jcode ({})", path.display())),
        Err(e) => skipped.push(format!("jcode: {e:#}")),
    }

    if quiet {
        if !synced.is_empty() && skipped.is_empty() {
            return Ok(());
        }
        if synced.is_empty() {
            anyhow::bail!("{}", skipped.join("; "));
        }
        for s in &skipped {
            eprintln!("agent sync partial: {s}");
        }
        return Ok(());
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "active": entry.name, "model_id": model_id,
                "base_url": base_url, "synced": synced, "skipped": skipped,
            }))?
        );
        return Ok(());
    }
    println!(
        "{} agents → {} [{}] ({})",
        output::cyan("●"),
        entry.name,
        model_id,
        output::dim(&base_url)
    );
    for s in &synced {
        println!("  {} {s}", output::ok("synced"));
    }
    for s in &skipped {
        println!("  {} {s}", output::dim("skipped:"));
    }
    Ok(())
}

fn sync_opencode(model_id: &str, base_url: &str, context: i64) -> Result<std::path::PathBuf> {
    let path = opencode_path();
    if !path.exists() {
        anyhow::bail!("no config at {}", path.display());
    }
    let raw = std::fs::read_to_string(&path)?;
    let mut doc: serde_json::Value = serde_json::from_str(&raw)?;
    let provider = doc
        .pointer_mut("/provider/ai-local")
        .context("no provider.ai-local entry (create one first)")?;
    if let Some(url) = provider.pointer_mut("/options/baseURL") {
        *url = serde_json::json!(base_url);
    }
    if let Some(models) = provider.pointer_mut("/models") {
        if let Some(obj) = models.as_object_mut() {
            // Replace the whole model map: one entry for the active model.
            let name = format!("{} (modelsdesk)", model_id);
            *obj = [(
                model_id.to_string(),
                serde_json::json!({
                    "name": name,
                    "reasoning": false,
                    "tool_call": true,
                    "attachment": false,
                    "limit": { "context": context, "output": 8192 },
                }),
            )]
            .into_iter()
            .collect();
        }
    }
    std::fs::write(&path, serde_json::to_string_pretty(&doc)?)?;
    Ok(path)
}

fn sync_jcode(model_id: &str, base_url: &str) -> Result<std::path::PathBuf> {
    let path = jcode_path();
    if !path.exists() {
        anyhow::bail!("no config at {}", path.display());
    }
    let raw = std::fs::read_to_string(&path)?;
    let mut doc: toml::Value = raw.parse().context("parsing jcode config.toml")?;
    let provider = doc
        .get_mut("providers")
        .and_then(|p| p.get_mut("lfm-local"))
        .context("no providers.lfm-local profile (create with `jcode provider add` first)")?;
    if let Some(table) = provider.as_table_mut() {
        table.insert(
            "base_url".to_string(),
            toml::Value::String(base_url.to_string()),
        );
        table.insert(
            "default_model".to_string(),
            toml::Value::String(model_id.to_string()),
        );
        table.insert(
            "models".to_string(),
            toml::Value::Array(vec![toml::Value::Table(
                [(
                    "id".to_string(),
                    toml::Value::String(model_id.to_string()),
                )]
                .into_iter()
                .collect(),
            )]),
        );
    }
    std::fs::write(&path, toml::to_string(&doc)?)?;
    Ok(path)
}
