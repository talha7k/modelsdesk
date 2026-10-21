//! Agent integrations: write the active modelsdesk server into coding-agent
//! configs (`modelsd connect <agent>`).
//!
//! Supported agents:
//! - `jcode`   — ~/.jcode/config.toml  ([providers.modelsdesk] profile)
//! - `codex`   — ~/.codex/config.toml  ([model_providers.modelsdesk])
//! - `opencode`— ~/.config/opencode/opencode.json (custom provider)
//!
//! All writers are idempotent (re-running replaces the modelsdesk-managed
//! block/keys) and never touch unrelated config entries. The `write_*`
//! functions take explicit paths so tests can target a tempdir.

use anyhow::{Context, Result};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};

use crate::config::Config;

pub const AGENTS: &[&str] = &["jcode", "codex", "opencode"];

pub fn supported(agent: &str) -> bool {
    AGENTS.contains(&agent)
}

/// Everything a writer needs, derived from the active profile + config.
pub struct Endpoint {
    pub base_url: String,
    /// Model id to advertise (what GET /v1/models reports, or profile name).
    pub model_id: String,
    pub model_name: String,
}

impl Endpoint {
    pub fn from_config(cfg: &Config) -> Result<Self> {
        let model = cfg
            .active_entry()
            .context("no active profile — run `modelsd use <model>` first")?;
        Ok(Endpoint {
            base_url: format!("http://{}:{}/v1", cfg.host, cfg.port),
            model_id: model.name.clone(),
            model_name: model.repo.clone(),
        })
    }
}

// ---------- jcode: ~/.jcode/config.toml ----------
// Writes/updates [providers.modelsdesk] and sets [provider] defaults, using
// the same profile shape `jcode provider add` produces. Local server: no key.

pub fn jcode_config_path() -> PathBuf {
    if let Ok(home) = std::env::var("JCODE_HOME") {
        return PathBuf::from(home).join("config.toml");
    }
    crate::platform::home().join(".jcode").join("config.toml")
}

pub fn write_jcode(path: &Path, ep: &Endpoint) -> Result<()> {
    let mut doc = read_toml(path)?;
    let providers = doc
        .entry("providers")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let table = providers
        .as_table_mut()
        .context("[providers] is not a table in jcode config")?;
    let mut md = toml_edit::Table::new();
    md["base_url"] = toml_edit::value(ep.base_url.clone());
    md["no_api_key"] = toml_edit::value(true);
    let mut models = toml_edit::Table::new();
    let mut m = toml_edit::Table::new();
    m["name"] = toml_edit::value(ep.model_name.clone());
    models.insert(
        &toml_edit::Key::new(&ep.model_id),
        toml_edit::Item::Table(m),
    );
    md["models"] = toml_edit::Item::Table(models);
    table.insert(
        &toml_edit::Key::new("modelsdesk"),
        toml_edit::Item::Table(md),
    );

    // Point [provider] defaults at the modelsdesk profile.
    let provider = doc
        .entry("provider")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let ptable = provider
        .as_table_mut()
        .context("[provider] is not a table in jcode config")?;
    ptable["default_provider"] = toml_edit::value("modelsdesk");
    ptable["model"] = toml_edit::value(ep.model_id.clone());

    write_toml(path, &doc)
}

// ---------- codex: ~/.codex/config.toml ----------
// [model_providers.modelsdesk] with wire_api = "responses". Local servers
// need no env_key.

pub fn codex_config_path() -> PathBuf {
    crate::platform::home().join(".codex").join("config.toml")
}

pub fn write_codex(path: &Path, ep: &Endpoint) -> Result<()> {
    let mut doc = read_toml(path)?;
    let mp = doc
        .entry("model_providers")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let table = mp
        .as_table_mut()
        .context("[model_providers] is not a table in codex config")?;
    let mut md = toml_edit::Table::new();
    md["name"] = toml_edit::value("modelsdesk (local)");
    md["base_url"] = toml_edit::value(ep.base_url.clone());
    md["wire_api"] = toml_edit::value("responses");
    table.insert(
        &toml_edit::Key::new("modelsdesk"),
        toml_edit::Item::Table(md),
    );

    doc["model"] = toml_edit::value(ep.model_id.clone());
    doc["model_provider"] = toml_edit::value("modelsdesk");

    write_toml(path, &doc)
}

// ---------- opencode: ~/.config/opencode/opencode.json ----------
// provider.modelsdesk custom entry with @ai-sdk/openai-compatible.

pub fn opencode_config_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join("opencode").join("opencode.json");
    }
    crate::platform::home()
        .join(".config")
        .join("opencode")
        .join("opencode.json")
}

pub fn write_opencode(path: &Path, ep: &Endpoint) -> Result<()> {
    let mut doc: Json = if path.exists() {
        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?
    } else {
        Json::Object(serde_json::Map::new())
    };
    let obj = doc
        .as_object_mut()
        .context("opencode config root must be a JSON object")?;
    if !obj.contains_key("$schema") {
        obj.insert(
            "$schema".to_string(),
            Json::String("https://opencode.ai/config.json".to_string()),
        );
    }
    let provider = obj
        .entry("provider")
        .or_insert_with(|| Json::Object(serde_json::Map::new()));
    let pobj = provider
        .as_object_mut()
        .context("\"provider\" must be an object in opencode config")?;
    let model_id = ep.model_id.clone();
    let model_name = ep.model_name.clone();
    pobj.insert(
        "modelsdesk".to_string(),
        serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": "modelsdesk (local)",
            "options": { "baseURL": ep.base_url },
            "models": {
                model_id: { "name": model_name }
            }
        }),
    );
    // Default the agent to the local model.
    obj.insert(
        "model".to_string(),
        serde_json::json!(format!("modelsdesk/{}", ep.model_id)),
    );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

// ---------- dispatch ----------

pub fn config_path_for(agent: &str) -> Result<PathBuf> {
    match agent {
        "jcode" => Ok(jcode_config_path()),
        "codex" => Ok(codex_config_path()),
        "opencode" => Ok(opencode_config_path()),
        other => anyhow::bail!("unknown agent '{other}' (supported: {})", AGENTS.join(", ")),
    }
}

pub fn write_for(agent: &str, path: &Path, ep: &Endpoint) -> Result<()> {
    match agent {
        "jcode" => write_jcode(path, ep),
        "codex" => write_codex(path, ep),
        "opencode" => write_opencode(path, ep),
        other => anyhow::bail!("unknown agent '{other}' (supported: {})", AGENTS.join(", ")),
    }
}

// ---------- toml helpers ----------

fn read_toml(path: &Path) -> Result<toml_edit::DocumentMut> {
    if !path.exists() {
        return Ok(toml_edit::DocumentMut::new());
    }
    let raw = std::fs::read_to_string(path)?;
    raw.parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("parsing {}", path.display()))
}

fn write_toml(path: &Path, doc: &toml_edit::DocumentMut) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, doc.to_string())?;
    Ok(())
}
