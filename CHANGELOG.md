# Changelog

All notable changes to modelsdesk are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions are
semver.

## [Unreleased]

### Added

- `modelsdesk connect <agent>`: write the active local server into coding-agent
  configs — `jcode` (~/.jcode/config.toml provider profile), `codex`
  (~/.codex/config.toml model_providers with wire_api = "responses"), and
  `opencode` (~/.config/opencode/opencode.json custom provider). Idempotent,
  preserves unrelated config, `--dry-run` and `--list` supported. This is the
  0.3.0 agent-integration feature.

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
