---
name: modeldesk
description: >-
  Manage local LLM servers with the modeldesk CLI: search Hugging Face,
  set up models end-to-end, start/stop/health, switch models, sync agent
  configs, check backend/model freshness. Use when the user runs models
  locally, asks about local inference, needs a model server for
  OpenCode/jcode/agents, or mentions modeldesk, mlx-dspark, llama.cpp,
  ollama, or local model health.

  Trigger phrases include "local model", "local llm", "start the model",
  "model server", "model health", "switch model", "which models fit",
  "find a model", "model outdated", "update the backend", "modeldesk"
---

# ModelDesk

`modelsd` (the modelsdesk CLI) is a Rust CLI that manages local LLM inference servers —
macOS first (MLX + mlx-dspark), provider-agnostic core (llama.cpp,
Ollama work cross-platform).

- **Install:** `cargo install modelsdesk` (crates.io) or
  `cargo install --path .` from https://github.com/talha7k/modelsdesk
- **Source:** https://github.com/talha7k/modelsdesk (GPL-3.0-or-later)

## Command map

| Task | Command |
|---|---|
| Find runnable models on HF | `modelsd search <terms> [--limit N]` |
| Set up end-to-end (verify → install → download → serve) | `modelsd setup <repo> [--name N] [--provider ID] [--drafter R] [--start]` |
| What's installed (+versions) | `modelsd providers` |
| What's downloaded | `modelsd models` |
| Validate before launch | `modelsd doctor [name] [--fix]` |
| Start (validated, auto-installs backend) | `modelsd start [--foreground] [--no-install]` |
| Stop | `modelsd stop` |
| Restart | `modelsd restart` |
| Health probe | `modelsd health` (alias `status`) |
| Switch model (hot-swap + agent re-sync) | `modelsd use <name> [--provider <id>] [--restart]` |
| Pre-fetch weights | `modelsd pull [name]` |
| Backend + model freshness | `modelsd outdated` |
| Sync OpenCode/jcode to active model | `modelsd sync-agents` |
| Live console | `modelsd watch` (q to quit) |
| Logs | `modelsd logs [n] [--follow]` |
| Profiles | `modelsd profile <list\|show\|add\|remove>` |
| Completions | `modelsd completions <bash\|zsh\|fish\|powershell\|elvish>` |

Append `--json` to health/models/providers/use/doctor/search/outdated/sync-agents
for machine output. Append `--verbose` for stderr diagnostics.
`MODELDESK_NO_INSTALL=1` disables auto-install everywhere.

## Setup flow (the happy path)

```bash
modelsd search qwen3 8b                 # annotated HF hits
modelsd setup mlx-community/Qwen3-8B-8bit --name qwen3-8b --start
modeldesk health
```

`setup` verifies the repo exists on HF, recommends (or takes) the backend,
installs the backend when missing (brew/pipx, with progress), runs the full
doctor validation, downloads weights, saves the profile, and optionally
starts serving. `search` annotates each hit with format, recommended backend,
backend install status, RAM estimate vs this machine, and cache state.

## Backends and auto-install

| Backend | Weights | Platforms | Install (automatic) |
|---|---|---|---|
| `mlx-dspark` | MLX (+ DSpark drafter) | macOS arm64, macOS 14+ | `pipx install mlx-dspark` |
| `mlx-lm` | MLX | macOS arm64, macOS 14+ | `brew install mlx-lm` |
| `llama-cpp` | GGUF (auto-downloaded) | macOS/Linux/Windows | brew / apt / dnf / pacman / winget / choco |
| `ollama` | registry names | macOS/Linux/Windows | brew / install script / winget |
| `strata` | own tiers (q2_0, iq3_s, gyro-s…) | macOS/Linux/Windows | `curl … install.sh \| sh` (Strata) |
| `mistral-rs` | GGUF + safetensors | macOS/Linux/Windows | `curl … mistralrs.dev/install.sh \| sh` |
| `vllm` | safetensors/GGUF | Linux/Windows (CUDA) | `pip install vllm` |
| `sglang` | safetensors/GGUF | Linux (CUDA) | `pip install 'sglang[all]'` |

`start`, `setup`, and `doctor --fix` install a missing backend automatically
(OS + distro detected at runtime). Minimum versions are gated (mlx-dspark
≥0.17, llama.cpp ≥b5000); older backends warn but still run. GGUF weights for
llama.cpp download into `~/.cache/modeldesk/gguf/` (Q4_K_M preferred).

## Agent integrations

`modelsd connect <jcode|codex|opencode>` writes the active server into the
agent's config (OpenAI-compatible provider + default model). `--list` shows
agents + config paths, `--dry-run` prints without writing. Restart the agent
afterwards to pick it up.

## Profiles (stored model configs)

Profiles live in `config.toml` (see `modelsd profile list` for the path).
Each profile: `name + provider + repo + drafter? + kv_bits + context_window? +
temperature? + mode?`.

```bash
modeldesk profile add lfm-2.5-8b LiquidAI/LFM2.5-8B-A1B-MLX-4bit \
  --drafter LiquidAI/LFM2.5-8B-A1B-DSpark
modeldesk profile add mistral-7b ./models/mistral-7b.Q4_K_M.gguf \
  --provider llama-cpp --context-window 8192
modeldesk use lfm-2.5-8b
```

`profile add` validates format/provider/arch **before saving** — a bad
combo is rejected with the reason, never stored.

## Freshness (outdated detection)

- `modelsd outdated`: live check of backend releases (PyPI / brew
  livecheck / GitHub releases) + cached-model staleness (HF `lastModified`
  vs cache mtime, 24h grace for metadata-only bumps).
- **Ambient warnings:** every other command prints cached `! outdated: …`
  stderr warnings when something is stale. Cache is 6 hours, refreshed by a
  detached background worker that never blocks or fails the command. Skipped
  for `outdated` itself, completions, and `--json` runs.

## Compatibility rules (also in `references/compatibility.md`)

- Format is detected from the repo name: `-MLX-`/`mlx-` → MLX,
  `.gguf`/`-GGUF` → GGUF, else Safetensors/Unknown.
- `mlx-dspark` / `mlx-lm`: MLX weights only. Apple Silicon + macOS 14+ only.
- `llama-cpp`: GGUF only. `ollama`: own registry names, `ollama pull` first.
- RAM is estimated from param count × quant + drafter + overhead;
  `doctor` fails above 85% of machine RAM, warns above 60%.
- Hot-swap (`POST /admin/load`) is same-backend only (mlx-dspark).
  A provider change always does stop + full restart.

## Wiring agents to the server

The server is OpenAI-compatible at `http://127.0.0.1:<port>/v1`
(default port 8090) plus an Anthropic endpoint at `/v1/messages`.

- **OpenCode** (`~/.config/opencode/opencode.json`): provider with
  `"npm": "@ai-sdk/openai-compatible"`, `"baseURL": "http://127.0.0.1:8090/v1"`,
  model id = the served repo short name.
- **jcode**: `jcode provider add <name> --base-url http://127.0.0.1:8090/v1
  --model <model-id> --api-key-stdin` (any dummy key; the server ignores it,
  but jcode's discovery probe requires the header). Note: jcode `auth-test`
  FAILs on its `/v1/models` probe (no bearer sent) while chat completions
  work — verify with a real `run`, not `auth-test`.

`modelsd sync-agents` rewrites both configs to the active profile;
`modelsd use` runs it automatically after every switch.

## Safety rules

- `stop` matches backend binaries on modeldesk's port only. A non-backend
  process (Expo, another tool) can never match — it is reported by name,
  never killed blindly.
- `doctor` runs before every `start` when the setup changed.
- Spinners degrade to plain lines when piped — scripts always see results.
- Memory pressure on 16 GB Macs is normal with 8B models — suggest closing
  apps or a smaller quant before blaming the server.
- Reasoning models put thinking in `reasoning_content`, leaving `content`
  empty on short replies — agents expecting `content` may show blanks.
