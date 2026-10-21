# Changelog

All notable changes to modelsdesk are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions are
semver.

## [Unreleased]

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
