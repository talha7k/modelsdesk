# ModelDesk

**One CLI to find, install, serve, and monitor local LLMs — without the glue scripts.**

ModelDesk manages the full lifecycle of running large language models on your own
machine: discovering models on Hugging Face, validating they fit your hardware,
downloading weights, launching the right inference backend, health-checking the
server, switching between models, and keeping both backends and cached weights
fresh. macOS first (MLX + speculative decoding), provider-agnostic core for
everywhere else.

```bash
modeldesk search qwen3 8b          # find runnable models on Hugging Face
modeldesk setup mlx-community/Qwen3-8B-8bit --start   # validate → download → serve
modeldesk health                   # is it answering?
```

## Why ModelDesk exists

Running a local model in 2026 means juggling five separate concerns, each with its
own tool and failure modes:

1. **Discovery** — Hugging Face hosts millions of repos in a dozen mutually
   incompatible weight formats (MLX, GGUF, AWQ, safetensors…). Picking the wrong
   file for your backend wastes a multi-gigabyte download.
2. **Compatibility** — MLX needs Apple Silicon. GGUF needs llama.cpp or Ollama.
   An 8B model in bf16 needs ~19 GB of RAM; the same model in 4-bit needs ~6 GB.
   Nobody tells you this *before* the OOM kill.
3. **Serving** — each backend (`mlx-dspark`, `mlx-lm`, `llama-server`, `ollama`)
   has different flags, health endpoints, and admin APIs. Shell aliases rot.
4. **Switching** — changing models means killing the server, editing agent
   configs (OpenCode, Claude Code wrappers, jcode…), and restarting. Every
   switch is a chance to break your tooling.
5. **Freshness** — backends ship weekly; cached weights go stale. Nothing tells
   you `mlx-lm 0.32.0 → 0.32.2` exists until something breaks.

ModelDesk collapses all five into one typed, tested Rust binary: `search`
annotates every Hugging Face hit with format, recommended backend, install
status, RAM estimate, and cache state. `setup` chains verify → validate →
download → profile → serve. `doctor` fails fast with actionable hints *before*
anything downloads. `use` hot-swaps models without restarting and re-syncs your
agent configs. `outdated` plus ambient 6-hour warnings keep backends and weights
current.

**Non-goals:** ModelDesk is a lifecycle manager, not an inference engine. It
orchestrates best-in-class backends; it doesn't reimplement sampling, KV
caches, or speculative decoding.

## Get started in 60 seconds

**Prerequisites:** macOS on Apple Silicon (Linux/Windows work for llama.cpp and
Ollama backends), Rust toolchain (`brew install rust`), Homebrew.

```bash
# 1. Install
cargo install --path .
modeldesk completions zsh > ~/.zsh/completions/_modeldesk

# 2. See what's available
modeldesk providers   # backends: installed? which version? supported here?
modeldesk models      # weights already in your HF cache

# 3. Find a model that fits this machine
modeldesk search "LFM 8b" --limit 5

# 4. Set it up end-to-end (validates, downloads, saves profile, serves)
modeldesk setup LiquidAI/LFM2.5-8B-A1B-MLX-4bit \
  --drafter LiquidAI/LFM2.5-8B-A1B-DSpark --start

# 5. Confirm it's answering
modeldesk health
```

Point any OpenAI-compatible client at `http://127.0.0.1:8090/v1` — OpenCode,
Claude Code wrappers, plain `curl`. An Anthropic-compatible endpoint
lives at `/v1/messages` on backends that provide one (mlx-dspark).

## Commands

| Command | What it does |
|---|---|
| `search <terms> [--limit N]` | Hugging Face search annotated with format, backend, fit, cache state |
| `setup <repo> [--name N] [--provider ID] [--drafter R] [--start]` | Verify → validate → download → profile, optionally serve |
| `start [--foreground] [--no-install]` | Validate profile, auto-install backend if missing, launch with progress |
| `stop` | Stop the server on our port (backend processes only — foreign holders are named, never killed) |
| `restart` | Stop + start |
| `health` / `status` | Process + endpoint probe (`--json` supported) |
| `models` | Local cache scan with real sizes + provider registry fit |
| `use <name> [--provider ID] [--restart]` | Switch profile; hot-swap same-backend, restart on provider change, re-sync agents |
| `providers` | Backend binaries, versions, minimum gates, install hints |
| `pull [name]` | Pre-fetch weights (`ollama pull`; others download on first start) |
| `doctor [name] [--fix]` | provider / format / arch / RAM / port checks; `--fix` installs the backend |
| `outdated` | Live check: backend releases + cached-model staleness |
| `profile <list\|show\|add\|remove>` | Stored model configs (validated before save) |
| `sync-agents` | Rewrite OpenCode/jcode configs to the active model |
| `logs [n] [--follow]` | Server log tail |
| `watch` | Full-screen live console (q to quit) |
| `completions <shell>` | Shell completions (bash, zsh, fish, powershell, elvish) |

Append `--json` to `health`, `models`, `providers`, `use`, `doctor`,
`search`, `outdated`, and `sync-agents` for machine-readable output.
`--verbose` adds stderr diagnostics. `MODELDESK_NO_INSTALL=1` disables
auto-install (fail with the install command instead).

## How it works

**Profiles** (`modeldesk profile add`) are named configurations — repo +
provider + drafter + tuning (`kv_bits`, `context_window`, `temperature`,
`mode`) — stored in `config.toml`. Compatibility is enforced *before* saving:
format/provider/arch/RAM are validated, and a bad profile is rejected with the
reason, never stored.

**Providers** are a trait (`src/providers.rs`): `mlx-dspark` (DSpark/DFlash
speculative decoding on Apple Silicon, drafter auto-resolution), `mlx-lm`
(plain MLX), `llama-cpp` (GGUF cross-platform, EAGLE draft models), `ollama`
(managed daemon), `strata` (Rust MoE engine), `mistral-rs` (Rust engine),
`vllm` + `sglang` (high-throughput Linux/CUDA), `tabby` (TabbyAPI/ExllamaV3
for consumer GPUs). Adding a backend means implementing eight methods —
binary discovery, launch args, health, admin hooks, version probe, weight
fetch.

**Freshness** has two layers: `modeldesk outdated` does a live check of backend
releases (PyPI / brew livecheck / GitHub releases) and cached-model staleness
(HF `lastModified` vs cache mtime, 24h grace for metadata-only bumps). Every
other command prints cached warnings to stderr when something is outdated —
cache is 6 hours, refreshed by a detached background worker that never blocks
or fails your command.

**Safety rules** the tool enforces on itself:

- `stop` matches backend binaries on *our* port only. An Expo dev server or
  another tool on a different port is invisible to it; a foreign holder on our
  port is reported by name, never killed blindly.
- Spinners degrade to plain `println!` lines when piped — scripts and logs
  always see what happened.
- `doctor` runs before every `start`: missing backend, wrong format, too-big
  model, and busy ports fail fast with the fix attached.

## Wiring coding agents

One command writes the active server into any agent's config:

```
modeldesk connect jcode     # ~/.jcode/config.toml provider profile
modeldesk connect codex     # ~/.codex/config.toml (wire_api=responses)
modeldesk connect opencode  # ~/.config/opencode/opencode.json + lean agent
modeldesk connect zed       # ~/.config/zed/settings.json
modeldesk connect cline     # ~/.cline/data/settings/providers.json
modeldesk connect goose     # ~/.config/goose/custom_providers/*.json
modeldesk connect crush     # ~/.config/crush/crushrc
modeldesk connect aider     # ~/.aider.conf.yml
modeldesk connect continue  # ~/.continue/config.yaml
```

All writers are idempotent and preserve unrelated config. `--list` shows
config paths; `--dry-run` prints without writing. Cursor can't be automated
(requests route via api2.cursor.sh, localhost unreachable); avante.nvim
config is inline Lua — both explain themselves in `connect --list`.

The server also speaks OpenAI (`/v1`) and Anthropic (`/v1/messages`)
protocols directly for anything else.

`modeldesk sync-agents` rewrites the legacy pair (OpenCode + jcode) to the
active profile, and `modeldesk use` runs it automatically after every
switch.

## Development

```bash
cargo build            # debug binary → ./target/debug/modeldesk
cargo test             # integration suite (assert_cmd + isolated HOME)
cargo build --release  # optimized binary (~6 MB, stripped)
```

Architecture: thin `main.rs` dispatch → `watch` (progress/TUI) → `server`
(process control) → `providers/*` (backends) with `compat` (validation),
`hub` (Hugging Face), `freshness` (updates), `agents` (config sync), and
`models` (cache scan) as supporting modules. Business logic lives in the
library crate so the CLI, TUI, and tests share it.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
