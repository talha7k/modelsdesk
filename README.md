# ModelDesk

Local LLM server manager — macOS first (MLX + mlx-dspark), provider-agnostic
core (llama.cpp, Ollama work wherever their binaries run).

## Install

```bash
cargo install --path .
modeldesk completions zsh > ~/.zsh/completions/_modeldesk
```

## Quick start

```bash
modeldesk providers   # what backends are installed
modeldesk models      # what weights are downloaded
modeldesk doctor      # validate active profile before launch
modeldesk start       # launch with animated progress
modeldesk health      # is it answering?
modeldesk watch       # live console (q to quit)
```

## Commands

| Command | What |
|---|---|
| `start [--foreground]` | Validate profile, launch backend, wait for health |
| `stop` | Stop only modeldesk's own server; foreign port holders are named, never killed |
| `restart` | Stop + start |
| `health` / `status` | Process + endpoint probe (`--json` supported) |
| `models` | HF/LM-Studio cache scan + registry fit |
| `use <name> [--provider ID] [--restart]` | Switch profile; hot-swap same-backend, restart on provider change |
| `providers` | Backend binaries, platforms, install hints |
| `pull [name]` | Pre-fetch weights (`ollama pull`; others download on first start) |
| `doctor [name]` | provider / format / arch / RAM / port checks |
| `profile <list\|show\|add\|remove>` | Stored model configs (validated before save) |
| `logs [n] [--follow]` | Server log tail |
| `watch` | Full-screen live console |
| `completions <shell>` | Shell completions |

## Profiles

`modeldesk profile add <name> <repo> [--provider ID] [--drafter REPO]
[--kv-bits 0|4|8] [--context-window N] [--temperature T] [--mode M]
[--overwrite]`

Compatibility (`src/compat.rs`) is enforced before save: MLX weights →
mlx backends (macOS/arm64 only), GGUF → llama-cpp/ollama, RAM estimate vs
machine (fail >85%, warn >60%).

## Providers

| Backend | Weights | Platforms | Install |
|---|---|---|---|
| `mlx-dspark` | MLX (+ DSpark drafter) | macOS arm64 | `pipx install mlx-dspark` |
| `mlx-lm` | MLX | macOS arm64 | `brew install mlx-lm` |
| `llama-cpp` | GGUF | macOS/Linux/Windows | `brew install llama.cpp` |
| `ollama` | registry names | macOS/Linux/Windows | `brew install ollama` |

## Wiring agents

OpenAI-compatible at `http://127.0.0.1:<port>/v1` (+ Anthropic `/v1/messages`).

- **OpenCode**: `ai-local`-style provider, `baseURL` → the server, model id → served name.
- **jcode**: `jcode provider add <name> --base-url …/v1 --model <id> --api-key-stdin`
  (dummy key; server ignores it but discovery needs the header).
