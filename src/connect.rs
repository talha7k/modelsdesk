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

pub const AGENTS: &[&str] = &["jcode", "codex", "opencode", "zed"];

/// Cursor cannot be configured by file: it routes chat/agent requests through
/// api2.cursor.sh, so a localhost endpoint is unreachable from it anyway, and
/// the only override ("Override OpenAI Base URL") lives in the GUI settings,
/// not on disk.
pub const UNSUPPORTED: &[(&str, &str)] = &[("cursor", "Cursor routes via api2.cursor.sh and has no file-based provider config; localhost endpoints are unreachable from it")];

pub fn supported(agent: &str) -> bool {
    AGENTS.contains(&agent)
}

/// Everything a writer needs, derived from the active profile + config.
pub struct Endpoint {
    pub base_url: String,
    /// Model id to advertise (what GET /v1/models reports, or profile name).
    pub model_id: String,
    pub model_name: String,
    /// Optional system prompt for agents that support one (opencode).
    pub prompt: Option<String>,
}

/// A lean prompt tuned for small local models: agents configured with the
/// user's orchestration prompts (built for frontier models) overwhelm local
/// weights with huge instruction payloads.
pub const LOCAL_MODEL_PROMPT: &str = "You are a focused coding assistant running on a small local model. \
Keep answers short and concrete. Prefer exact file paths, code, and commands over prose. \
If a task is too large, do the first concrete step and say what's next instead of planning at length.";

impl Endpoint {
    pub fn from_config(cfg: &Config) -> Result<Self> {
        let model = cfg
            .active_entry()
            .context("no active profile — run `modelsd use <model>` first")?;
        Ok(Endpoint {
            base_url: format!("http://{}:{}/v1", cfg.host, cfg.port),
            model_id: model.name.clone(),
            model_name: model.repo.clone(),
            prompt: None,
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
    // Match the profile shape `jcode provider add` writes:
    // [providers.<name>] + [[providers.<name>.models]] (array of tables).
    // Remove any stale modelsdesk tables first so re-runs replace cleanly.
    if let Some(providers) = doc.get_mut("providers").and_then(|p| p.as_table_mut()) {
        providers.remove("modelsdesk");
    }
    let providers = doc
        .entry("providers")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let table = providers
        .as_table_mut()
        .context("[providers] is not a table in jcode config")?;
    let mut md = toml_edit::Table::new();
    // Exact shape `jcode provider add --no-api-key` writes.
    md["type"] = toml_edit::value("openai-compatible");
    md["base_url"] = toml_edit::value(ep.base_url.clone());
    md["auth"] = toml_edit::value("none");
    md["default_model"] = toml_edit::value(ep.model_id.clone());
    md["requires_api_key"] = toml_edit::value(false);
    md["models"] = toml_edit::Item::ArrayOfTables({
        let mut aot = toml_edit::ArrayOfTables::new();
        let mut m = toml_edit::Table::new();
        m["id"] = toml_edit::value(ep.model_id.clone());
        aot.push(m);
        aot
    });
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
    ptable["default_model"] = toml_edit::value(ep.model_id.clone());

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
    let provider_model_key = model_id.clone();
    let model_name = ep.model_name.clone();
    let prompt = ep.prompt.clone().unwrap_or_else(|| LOCAL_MODEL_PROMPT.to_string());
    pobj.insert(
        "modelsdesk".to_string(),
        serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": "modelsdesk (local)",
            "options": { "baseURL": ep.base_url },
            "models": {
                provider_model_key: { "name": model_name }
            }
        }),
    );
    // A dedicated lean agent for the local model. The user's existing agents
    // carry orchestration prompts sized for frontier models; on local weights
    // those blow the context and stall generation.
    let agents = obj
        .entry("agent".to_string())
        .or_insert_with(|| Json::Object(serde_json::Map::new()));
    let aobj = agents
        .as_object_mut()
        .context("\"agent\" must be an object in opencode config")?;
    let agent_model = format!("modelsdesk/{model_id}");
    aobj.insert(
        "modelsdesk-local".to_string(),
        serde_json::json!({
            "model": agent_model,
            "prompt": prompt,
            "description": "Lean agent for small local models (modelsdesk)"
        }),
    );
    // Default the agent + model to the local ones.
    obj.insert("default_agent".to_string(), Json::String("modelsdesk-local".to_string()));
    let default_model = format!("modelsdesk/{}", ep.model_id);
    obj.insert("model".to_string(), Json::String(default_model));

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

// ---------- zed: ~/.config/zed/settings.json ----------
// language_models.openai_compatible.modelsdesk entry (JSONC-ish; Zed tolerates
// plain JSON). No API key needed for a local server.

pub fn zed_config_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join("zed").join("settings.json");
    }
    crate::platform::home()
        .join(".config")
        .join("zed")
        .join("settings.json")
}

pub fn write_zed(path: &Path, ep: &Endpoint) -> Result<()> {
    let mut doc: Json = if path.exists() {
        let raw = std::fs::read_to_string(path)?;
        // Zed settings are JSONC; strip // comments (but not // inside URLs
        // like http:// — only when preceded by whitespace or line start).
        let cleaned: String = raw
            .lines()
            .map(|l| {
                let bytes = l.as_bytes();
                let mut cut = None;
                for i in 0..bytes.len().saturating_sub(1) {
                    if bytes[i] == b'/' && bytes[i + 1] == b'/' {
                        let ok = i == 0 || bytes[i - 1].is_ascii_whitespace();
                        if ok {
                            cut = Some(i);
                            break;
                        }
                    }
                }
                match cut {
                    Some(i) => &l[..i],
                    None => l,
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        serde_json::from_str(&cleaned).with_context(|| format!("parsing {}", path.display()))?
    } else {
        Json::Object(serde_json::Map::new())
    };
    let obj = doc
        .as_object_mut()
        .context("zed settings root must be a JSON object")?;
    let lms = obj
        .entry("language_models".to_string())
        .or_insert_with(|| Json::Object(serde_json::Map::new()));
    let lm_obj = lms
        .as_object_mut()
        .context("\"language_models\" must be an object in zed settings")?;
    let oc = lm_obj
        .entry("openai_compatible".to_string())
        .or_insert_with(|| Json::Object(serde_json::Map::new()));
    let oc_obj = oc
        .as_object_mut()
        .context("\"openai_compatible\" must be an object in zed settings")?;
    oc_obj.insert(
        "modelsdesk".to_string(),
        serde_json::json!({
            "api_url": ep.base_url,
            "available_models": [{
                "name": ep.model_id,
                "display_name": ep.model_name,
                "max_tokens": 32768,
                "capabilities": {
                    "tools": false,
                    "images": false,
                    "parallel_tool_calls": false,
                    "prompt_cache_key": false,
                    "chat_completions": true,
                    "interleaved_reasoning": false,
                    "max_tokens_parameter": true
                }
            }]
        }),
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
        "zed" => Ok(zed_config_path()),
        other => {
            if let Some((_, why)) = UNSUPPORTED.iter().find(|(n, _)| *n == other) {
                anyhow::bail!("{other} is not supported: {why}");
            }
            anyhow::bail!("unknown agent '{other}' (supported: {})", AGENTS.join(", "))
        }
    }
}

pub fn write_for(agent: &str, path: &Path, ep: &Endpoint) -> Result<()> {
    match agent {
        "jcode" => write_jcode(path, ep),
        "codex" => write_codex(path, ep),
        "opencode" => write_opencode(path, ep),
        "zed" => write_zed(path, ep),
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
