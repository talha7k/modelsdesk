use modelsdesk::config::Config;
use modelsdesk::connect::{
    self, Endpoint, AGENTS,
};

fn ep() -> Endpoint {
    Endpoint {
        base_url: "http://127.0.0.1:8090/v1".to_string(),
        model_id: "lfm-2.5-8b".to_string(),
        model_name: "LiquidAI/LFM2.5-8B-A1B-MLX-4bit".to_string(),
        prompt: None,
    }
}

#[test]
fn endpoint_derives_from_config() {
    let cfg = Config::default();
    let ep = Endpoint::from_config(&cfg).unwrap();
    assert_eq!(ep.base_url, "http://127.0.0.1:8090/v1");
    assert_eq!(ep.model_id, "lfm-2.5-8b");
}

#[test]
fn agent_registry() {
    assert!(connect::supported("jcode"));
    assert!(connect::supported("codex"));
    assert!(connect::supported("opencode"));
    assert!(connect::supported("zed"));
    assert!(!connect::supported("cursor")); // documented as unsupported
    assert!(!connect::supported("lm-studio"));
    assert_eq!(AGENTS, &["jcode", "codex", "opencode", "zed"]);
    // Cursor gets a specific explanation, not a generic unknown-agent error.
    let err = connect::config_path_for("cursor").unwrap_err().to_string();
    assert!(err.contains("not supported"), "cursor error: {err}");
}

#[test]
fn jcode_writer_creates_and_preserves() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    connect::write_jcode(&path, &ep()).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    let doc: toml::Value = raw.parse().unwrap();
    // Shape must match `jcode provider add --no-api-key` exactly.
    assert_eq!(
        doc["providers"]["modelsdesk"]["type"].as_str().unwrap(),
        "openai-compatible"
    );
    assert_eq!(
        doc["providers"]["modelsdesk"]["base_url"]
            .as_str()
            .unwrap(),
        "http://127.0.0.1:8090/v1"
    );
    assert_eq!(
        doc["providers"]["modelsdesk"]["auth"].as_str().unwrap(),
        "none"
    );
    assert!(!doc["providers"]["modelsdesk"]["requires_api_key"]
        .as_bool()
        .unwrap());
    assert_eq!(
        doc["providers"]["modelsdesk"]["models"][0]["id"]
            .as_str()
            .unwrap(),
        "lfm-2.5-8b"
    );
    assert_eq!(
        doc["provider"]["default_provider"].as_str().unwrap(),
        "modelsdesk"
    );
    assert_eq!(
        doc["provider"]["default_model"].as_str().unwrap(),
        "lfm-2.5-8b"
    );

    // Pre-existing unrelated config must survive an idempotent re-run.
    let raw2 = format!(
        "{raw}\n[display]\ncentered = true\n\n[providers.other]\nbase_url = \"https://x.example/v1\"\n"
    );
    std::fs::write(&path, raw2).unwrap();
    let mut ep2 = ep();
    ep2.base_url = "http://10.0.0.5:9000/v1".to_string();
    connect::write_jcode(&path, &ep2).unwrap();
    let doc2: toml::Value = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(
        doc2["providers"]["modelsdesk"]["base_url"].as_str().unwrap(),
        "http://10.0.0.5:9000/v1"
    );
    assert_eq!(
        doc2["providers"]["other"]["base_url"].as_str().unwrap(),
        "https://x.example/v1"
    );
    assert!(doc2["display"]["centered"].as_bool().unwrap());
}

#[test]
fn codex_writer_creates_and_preserves() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    connect::write_codex(&path, &ep()).unwrap();
    let doc: toml::Value = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(
        doc["model_providers"]["modelsdesk"]["base_url"].as_str().unwrap(),
        "http://127.0.0.1:8090/v1"
    );
    assert_eq!(
        doc["model_providers"]["modelsdesk"]["wire_api"].as_str().unwrap(),
        "responses"
    );
    assert_eq!(doc["model_provider"].as_str().unwrap(), "modelsdesk");
    assert_eq!(doc["model"].as_str().unwrap(), "lfm-2.5-8b");

    // Unrelated keys survive.
    let raw = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{raw}\n[projects.\"/tmp\"]\ntrust_level = \"trusted\"\n")).unwrap();
    connect::write_codex(&path, &ep()).unwrap();
    let doc2: toml::Value = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert!(doc2["projects"]["/tmp"]["trust_level"].is_str());
}

#[test]
fn opencode_writer_creates_and_preserves() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.json");

    connect::write_opencode(&path, &ep()).unwrap();
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        doc["$schema"].as_str().unwrap(),
        "https://opencode.ai/config.json"
    );
    assert_eq!(
        doc["provider"]["modelsdesk"]["npm"].as_str().unwrap(),
        "@ai-sdk/openai-compatible"
    );
    assert_eq!(
        doc["provider"]["modelsdesk"]["options"]["baseURL"]
            .as_str()
            .unwrap(),
        "http://127.0.0.1:8090/v1"
    );
    assert_eq!(
        doc["provider"]["modelsdesk"]["models"]["lfm-2.5-8b"]["name"]
            .as_str()
            .unwrap(),
        "LiquidAI/LFM2.5-8B-A1B-MLX-4bit"
    );
    assert_eq!(doc["model"].as_str().unwrap(), "modelsdesk/lfm-2.5-8b");
    // Lean local agent with a prompt, set as default.
    assert!(doc["agent"]["modelsdesk-local"]["prompt"].is_string());
    assert_eq!(
        doc["agent"]["modelsdesk-local"]["model"].as_str().unwrap(),
        "modelsdesk/lfm-2.5-8b"
    );
    assert_eq!(
        doc["default_agent"].as_str().unwrap(),
        "modelsdesk-local"
    );

    // Unrelated provider + top-level keys + agents survive re-run.
    let mut doc2 = doc.clone();
    doc2["provider"]["ollama"] = serde_json::json!({"npm": "@ai-sdk/openai-compatible"});
    doc2["theme"] = serde_json::json!("dark");
    doc2["agent"]["explore"] = serde_json::json!({"model": "zai/x", "prompt": "p"});
    std::fs::write(&path, serde_json::to_string(&doc2).unwrap()).unwrap();
    connect::write_opencode(&path, &ep()).unwrap();
    let doc3: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(doc3["provider"]["ollama"].is_object());
    assert_eq!(doc3["theme"].as_str().unwrap(), "dark");
    assert!(doc3["provider"]["modelsdesk"].is_object());
    // Pre-existing agents (explore etc.) survive.
    assert!(doc3["agent"]["explore"].is_object());
    assert!(doc3["agent"]["modelsdesk-local"].is_object());
}

#[test]
fn unknown_agent_rejected() {
    assert!(connect::config_path_for("vim").is_err());
    assert!(connect::write_for("vim", std::path::Path::new("/tmp/x"), &ep()).is_err());
}

#[test]
fn zed_writer_creates_and_preserves() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");

    connect::write_zed(&path, &ep()).unwrap();
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let md = &doc["language_models"]["openai_compatible"]["modelsdesk"];
    assert_eq!(md["api_url"].as_str().unwrap(), "http://127.0.0.1:8090/v1");
    assert_eq!(
        md["available_models"][0]["name"].as_str().unwrap(),
        "lfm-2.5-8b"
    );
    assert_eq!(
        md["available_models"][0]["capabilities"]["chat_completions"]
            .as_bool()
            .unwrap(),
        true
    );

    // JSONC comments + unrelated keys survive re-run.
    let raw = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        format!("{{ // zed settings\n\"theme\": \"dark\", \"rest\": {raw} }}"),
    )
    .unwrap();
    connect::write_zed(&path, &ep()).unwrap();
    let doc2: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc2["theme"].as_str().unwrap(), "dark");
    assert!(doc2["rest"]["language_models"]["openai_compatible"]["modelsdesk"].is_object());
}
