# ModelDesk compatibility reference

How `modeldesk doctor` and `profile add` decide whether a model runs on a
backend. Same logic lives in `src/compat.rs` — this file is the human copy.

## Format detection (from repo string, lowercased)

| Signal | Format |
|---|---|
| ends with `.gguf`, contains `.gguf` or `-gguf` | GGUF |
| contains `-mlx`, `mlx-`, `/mlx-` | MLX |
| ends with `.safetensors` or contains `safetensors` | Safetensors |
| none of the above | Unknown (treated as MLX/Safetensors with a warning) |

## Provider matrix

| Backend | Accepts | Platforms | Notes |
|---|---|---|---|
| `mlx-dspark` | MLX (+ converted Safetensors from `LiquidAI/`, `mlx-community/`) | macOS arm64 only | DSpark speculative decoding; needs a drafter for speedup, runs without one in baseline |
| `mlx-lm` | MLX (+ converted Safetensors, same rule) | macOS arm64 only | Plain decoding, no speculation |
| `llama-cpp` | GGUF only | macOS, Linux, Windows | Needs a local `.gguf` file path or HF GGUF repo; `--ctx-size` from profile |
| `ollama` | Ollama registry names only | macOS, Linux, Windows | Managed daemon: `ollama pull <name>` first; modeldesk does not download for it |

## RAM estimation

`params × bytes-per-param + 1.5 GB overhead + 1.5 GB if drafter`

- Param count parsed from the repo name (`8b` → 8B, `2.6b` → 2.6B …; default 8B).
- Bytes/param: 4-bit/q4 → 0.55, 8-bit/q8 → 1.05, bf16/fp16/f16 → 2.1, default 1.05.
- `doctor` FAILS above 85% of machine RAM, WARNS above 60%.

## Hot-swap vs restart

- Same backend + provider supports `POST /admin/load` (mlx-dspark): hot-swap, port stays.
- Anything else (provider change, llama-cpp, ollama, mlx-lm): full stop + boot.
- `modeldesk use <name> --restart` forces the full cycle.

## Backend installation (runtime layer)

`start`, `setup`, and `doctor --fix` install a missing backend automatically
unless `--no-install` / `MODELDESK_NO_INSTALL=1`. OS, version, arch, distro,
and available package managers are detected at runtime (`src/runtime_env.rs`):

- macOS: brew → pipx → uv (MLX needs macOS 14+ on arm64, enforced)
- Linux: brew → pipx/uv → apt / dnf / pacman / zypper / apk by distro
- Windows: winget → choco → scoop

Minimum versions: mlx-dspark ≥0.17, llama.cpp ≥b5000. Older backends warn
(`old, want ≥X` in `providers`/`doctor`) but still run. Versions resolve via
binary → pipx → brew since MLX binaries have no `--version`.

## Freshness model

- Backends: PyPI (mlx-dspark, mlx-lm fallback) / `brew livecheck` (mlx-lm,
  ollama) / GitHub releases (llama.cpp, ollama fallback).
- Models: HF `lastModified` vs cache snapshot mtime, 24h grace for
  metadata-only bumps (prevents false "stale" on README edits).
- `outdated` is always live; ambient warnings use a 6h cache refreshed by a
  detached worker (`outdated --refresh-cache`, hidden flag).
