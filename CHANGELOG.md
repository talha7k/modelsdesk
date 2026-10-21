# Changelog

All notable changes to modelsdesk are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions are
semver.

## [Unreleased]

### Added

- New providers: `strata` (Rust MoE engine, consumer-GPU 125B-class models),
  `mistral-rs` (Rust engine, GGUF + safetensors), `vllm` (high-throughput
  server), `sglang` (fast serving engine). Install hints, freshness checks,
  and platform matrices updated for each.
- Release flow now requires and ships a changelog: `scripts/release.sh`
  verifies the version has a `CHANGELOG.md` entry before publishing, and the
  npm tarball includes `CHANGELOG.md`.
