use modelsdesk::config::ModelEntry;
use modelsdesk::providers::mlx_dspark::resolve_drafter;
use modelsdesk::providers::Provider;

fn entry(repo: &str, drafter: Option<&str>, mode: Option<&str>) -> ModelEntry {
    ModelEntry {
        name: "t".to_string(),
        provider: "mlx-dspark".to_string(),
        repo: repo.to_string(),
        drafter: drafter.map(|s| s.to_string()),
        kv_bits: 4,
        context_window: None,
        default_temperature: None,
        mode: mode.map(|s| s.to_string()),
    }
}

#[test]
fn drafter_auto_resolution() {
    // Known target, dspark: resolves the DSpark head.
    let (d, m) = resolve_drafter("openbmb/MiniCPM5-2B-MLX", "dspark");
    assert_eq!(d.as_deref(), Some("openbmb/MiniCPM5-2B-DSpark"));
    assert_eq!(m, "dspark");

    // Known quant variant still matches (prefix rule).
    let (d, _) = resolve_drafter("mlx-community/Qwen3-4B-8bit", "dspark");
    assert_eq!(d.as_deref(), Some("deepseek-ai/dspark_qwen3_4b_block7"));

    // dflash on a target WITH a DFlash head.
    let (d, m) = resolve_drafter("mlx-community/Qwen3-4B-8bit", "dflash");
    assert_eq!(d.as_deref(), Some("z-lab/Qwen3-4B-DFlash-b16"));
    assert_eq!(m, "dflash");

    // dflash on a target WITHOUT a DFlash head → lookup fallback.
    let (d, m) = resolve_drafter("openbmb/MiniCPM5-2B-MLX", "dflash");
    assert!(d.is_none());
    assert_eq!(m, "lookup");

    // Unknown target → lookup (drafter-free, any repo works).
    let (d, m) = resolve_drafter("some-org/random-model-mlx", "dspark");
    assert!(d.is_none());
    assert_eq!(m, "lookup");
}

#[test]
fn dspark_start_args_auto_drafter() {
    let p = modelsdesk::providers::mlx_dspark::MlxDspark;
    let e = entry("openbmb/MiniCPM5-2B-MLX", None, None); // no explicit drafter
    let args = p.start_args(&e, "127.0.0.1", 8090).unwrap().join(" ");
    assert!(args.contains("--drafter openbmb/MiniCPM5-2B-DSpark"), "{args}");
    assert!(args.contains("--mode dspark"), "{args}");

    // Explicit drafter wins over the table.
    let e = entry("openbmb/MiniCPM5-2B-MLX", Some("my/custom-drafter"), None);
    let args = p.start_args(&e, "127.0.0.1", 8090).unwrap().join(" ");
    assert!(args.contains("--drafter my/custom-drafter"), "{args}");

    // baseline never gets a drafter (--drafter flag, not --drafter-bits).
    let e = entry("openbmb/MiniCPM5-2B-MLX", None, Some("baseline"));
    let args = p.start_args(&e, "127.0.0.1", 8090).unwrap();
    assert!(!args.contains(&"--drafter".to_string()), "{args:?}");
    let joined = args.join(" ");
    assert!(joined.contains("--mode baseline"), "{joined}");
}

#[test]
fn llama_cpp_draft_args() {
    let p = modelsdesk::providers::llama_cpp::LlamaCpp;
    // Non-existent draft path → clear error (llama-server needs a file).
    let e = entry("model.gguf", Some("hf/repo-draft"), None);
    let err = p.start_args(&e, "127.0.0.1", 8090).unwrap_err().to_string();
    assert!(err.contains("local .gguf path"), "{err}");
}

#[test]
fn vllm_speculative_config() {
    let p = modelsdesk::providers::vllm::Vllm;
    let e = entry("meta-llama/Llama-4-70B", Some("meta-llama/Llama-4-70B-eagle3"), None);
    let args = p.start_args(&e, "0.0.0.0", 8000).unwrap().join(" ");
    assert!(args.contains("--speculative-config"), "{args}");
    assert!(args.contains("eagle3"), "{args}");
    assert!(args.contains("Llama-4-70B-eagle3"), "{args}");

    // ngram mode needs no model.
    let e = entry("m", Some("any"), Some("ngram"));
    let args = p.start_args(&e, "0.0.0.0", 8000).unwrap().join(" ");
    assert!(args.contains("prompt_lookup_max"), "{args}");

    // No drafter → no speculative args.
    let e = entry("m", None, None);
    let args = p.start_args(&e, "0.0.0.0", 8000).unwrap().join(" ");
    assert!(!args.contains("--speculative-config"), "{args}");
}

#[test]
fn stray_port_parsing() {
    // The exact cmdline of a real stray mlx-dspark on :8123.
    let cmd = "/opt/homebrew/Cellar/python@3.15/3.15.0/Frameworks/Python.app/Contents/MacOS/Python -E /Users/talha/.local/bin/mlx-dspark serve --model openbmb/MiniCPM5-2B-MLX --mode lookup --host 127.0.0.1 --port 8123";
    assert!(cmd.contains("mlx-dspark"));
    let mut it = cmd.split_whitespace();
    let mut port = None;
    while let Some(tok) = it.next() {
        if tok == "--port" {
            port = it.next().and_then(|v| v.parse::<u16>().ok());
        }
    }
    assert_eq!(port, Some(8123));
}
