# Changelog

All notable changes to modelsdesk are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions are
semver.

## [Unreleased]

## [0.3.0] - 2026-10-21

### Added

- Speculative decoding across all engine providers:
  - `mlx-dspark`: drafter auto-resolution — 17 known target→DSpark/DFlash
    drafter pairs embedded (verified against `mlx-dspark models`); profiles
    without a `drafter` now get one automatically (any quant variant
    matches by repo prefix), unknown targets fall back to drafter-free
    `lookup` mode, `mode = "dflash"` picks the DFlash head when one exists
  - `llama-cpp`: profile `drafter` now enables EAGLE-style speculative
    decoding via llama-server `--model-draft` (local .gguf draft models;
    clear error if the draft isn't a local file)
  - `vllm`: profile `drafter` + `mode` map to `--speculative-config`
    (eagle3 default; dspark/dflash/mtp/ngram supported, ngram is
    drafter-free)
- Live-verified: MiniCPM5-2B with auto-resolved DSpark head boots and
  serves without any drafter field on the profile.

- `modelsdesk connect <agent>`: write the active local server into coding-agent
  configs — `jcode` (~/.jcode/config.toml provider profile), `codex`
  (~/.codex/config.toml model_providers with wire_api = "responses"),
  `opencode` (~/.config/opencode/opencode.json custom provider + lean
  local-model agent), and `zed` (~/.config/zed/settings.json openai_compatible
  entry). Cursor is documented as unsupported (routes via api2.cursor.sh, no
  file-based config, localhost unreachable). Idempotent, preserves unrelated
  config, `--dry-run` and `--list` supported. Live-tested against jcode
  (round-trip ok) and opencode/zed (config accepted).

## [0.2.1] - 2026-10-21

### Added

- New providers: `strata` (Rust MoE engine, consumer-GPU 125B-class models),
  `mistral-rs` (Rust engine, GGUF + safetensors), `vllm` (high-throughput
  server), `sglang` (fast serving engine). Install hints, freshness checks,
  and platform matrices updated for each.
- Release flow now requires and ships a changelog: `scripts/release.sh`
  verifies the version has a `CHANGELOG.md` entry before publishing, and the
  npm tarball includes `CHANGELOG.md`.

### Fixed

- Test suite compiles again (wrong crate/binary names) — 9/9 pass.
- `release.sh` now bumps the `@modelsdesk/darwin-arm64` optionalDependency
  pin alongside the root version (0.2.0 shipped a stale 0.1.0 platform
  binary for this reason).

### Planned (0.3.0)

- Agent integrations: Jcode, Codex CLI, OpenCode — write provider config
  directly (`modelsdesk connect <agent>`).
