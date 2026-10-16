use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

fn isolated_home() -> TempDir {
    let dir = TempDir::new().unwrap();
    for sub in ["Library/Application Support", ".config", ".cache"] {
        fs::create_dir_all(dir.path().join(sub)).unwrap();
    }
    dir
}

fn cmd_with_home(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("modeldesk").unwrap();
    cmd.env("HOME", dir.path());
    cmd.env("XDG_CONFIG_HOME", dir.path().join(".config"));
    cmd.env("XDG_DATA_HOME", dir.path().join(".local/share"));
    cmd.env("HF_HOME", dir.path().join(".cache/huggingface"));
    cmd
}

#[test]
fn help_lists_core_commands() {
    let mut cmd = Command::cargo_bin("modeldesk").unwrap();
    cmd.arg("--help");
    let out = cmd.output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    for expected in [
        "start",
        "stop",
        "health",
        "models",
        "use",
        "providers",
        "doctor",
        "profile",
        "search",
        "setup",
        "sync-agents",
    ] {
        assert!(text.contains(expected), "help missing '{expected}'");
    }
}

#[test]
fn providers_json_shape() {
    let dir = isolated_home();
    let mut cmd = cmd_with_home(&dir);
    cmd.args(["providers", "--json"]);
    let out = cmd.output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("providers --json must be JSON");
    let arr = v.as_array().expect("top-level array");
    assert!(!arr.is_empty());
    for row in arr {
        assert!(row.get("id").is_some());
        assert!(row.get("status").is_some());
    }
    let ids: Vec<&str> = arr
        .iter()
        .filter_map(|r| r.get("id").and_then(|v| v.as_str()))
        .collect();
    for expected in ["mlx-dspark", "mlx-lm", "llama-cpp", "ollama"] {
        assert!(ids.contains(&expected), "missing provider {expected}");
    }
}

#[test]
fn doctor_rejects_unknown_profile() {
    let dir = isolated_home();
    let mut cmd = cmd_with_home(&dir);
    cmd.args(["doctor", "no-such-model"]);
    let out = cmd.output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("no-such-model"), "error names the model: {err}");
}

#[test]
fn use_rejects_unknown_provider() {
    let dir = isolated_home();
    let mut cmd = cmd_with_home(&dir);
    cmd.args(["use", "whatever", "--provider", "no-such-backend"]);
    let out = cmd.output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("no-such-backend"), "error names provider: {err}");
}

#[test]
fn profile_add_rejects_bad_combo() {
    let dir = isolated_home();
    let mut cmd = cmd_with_home(&dir);
    cmd.args([
        "profile",
        "add",
        "bad",
        "SomeOrg/SomeModel-MLX-8bit",
        "--provider",
        "llama-cpp",
    ]);
    let out = cmd.output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.contains("incompatible") || err.contains("cannot serve"),
        "rejects MLX on llama-cpp: {err}"
    );
}

#[test]
fn profile_add_and_show_roundtrip() {
    let dir = isolated_home();
    let mut cmd = cmd_with_home(&dir);
    cmd.args([
        "profile",
        "add",
        "tiny",
        "SomeOrg/Tiny-1B-MLX-4bit",
        "--provider",
        "mlx-lm",
    ]);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "add failed: {out:?}");

    let mut show = cmd_with_home(&dir);
    show.args(["profile", "show", "tiny", "--json"]);
    let out = show.output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v.get("name").and_then(|n| n.as_str()), Some("tiny"));
    assert_eq!(v.get("provider").and_then(|p| p.as_str()), Some("mlx-lm"));
}

#[test]
fn profile_add_duplicate_needs_overwrite() {
    let dir = isolated_home();
    let args = [
        "profile",
        "add",
        "dup",
        "SomeOrg/Dup-1B-MLX-4bit",
        "--provider",
        "mlx-lm",
    ];
    let mut first = cmd_with_home(&dir);
    first.args(&args);
    assert!(first.output().unwrap().status.success());

    let mut second = cmd_with_home(&dir);
    second.args(&args);
    let out = second.output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("already exists"), "dup guard: {err}");

    let mut third = cmd_with_home(&dir);
    third.args(&args).arg("--overwrite");
    assert!(third.output().unwrap().status.success());
}
