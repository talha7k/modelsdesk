use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "modelsdesk",
    version,
    about = "Manage local LLM servers — start/stop, health, models, providers",
    long_about = "ModelDesk manages local LLM inference servers on your machine.\n\
        \n\
        macOS first (MLX + mlx-dspark), provider-agnostic core: llama.cpp and\n\
        Ollama work wherever their binaries run, so Linux/Windows follow.\n\
        \n\
        QUICK START:\n    \
        modelsdesk providers                 # what backends are installed\n    \
        modelsdesk models                    # what weights are downloaded\n    \
        modelsdesk start                     # launch the active model\n    \
        modelsdesk health                    # is it answering?\n    \
        modelsdesk watch                     # live animated console\n\
        \n\
        Config lives at <config-dir>/modelsdesk/config.toml — edit it directly\n\
        or use `modelsdesk profile add`. Logs at <data-dir>/modelsdesk/logs/.",
    after_help = "EXAMPLES:\n    \
        modelsdesk search qwen3 8b\n    \
        modelsdesk setup mlx-community/Qwen3-8B-8bit --start\n    \
        modelsdesk start --foreground\n    \
        modelsdesk use qwen3-8b --provider llama-cpp\n    \
        modelsdesk outdated\n    \
        modelsdesk doctor\n    \
        modelsdesk health --json | jq .",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
    #[arg(
        long,
        global = true,
        help = "Emit machine-readable JSON where supported",
        long_help = "Emit machine-readable JSON instead of human output.\nSupported by: health, models, providers, use, doctor, search, setup, outdated, sync-agents."
    )]
    pub json: bool,
    #[arg(
        long,
        global = true,
        help = "Verbose diagnostics on stderr"
    )]
    pub verbose: bool,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(
        about = "Start the active model server",
        long_about = "Validate the active profile (provider installed, format\n\
            compatible, enough RAM, port free), auto-install a missing backend\n\
            (brew/pipx, unless --no-install), then launch with animated\n\
            progress. First run downloads weights — expect minutes.",
        after_help = "EXAMPLES:\n    \
            modelsdesk start\n    \
            modelsdesk start --foreground    # attach; Ctrl-C stops the server"
    )]
    Start {
        #[arg(long, help = "Run in foreground (attach to server output)")]
        foreground: bool,
        #[arg(
            long,
            help = "Don't auto-install a missing backend (fail with the install command instead)"
        )]
        no_install: bool,
    },
    #[command(
        about = "Stop the running model server",
        long_about = "Stop the inference server on the configured port. Matches\n\
            backend binaries (mlx-dspark, mlx_lm, llama-server, ollama) on our\n\
            port — regardless of who launched them. A non-backend process on\n\
            the port (Expo, another tool) can never match and is reported by\n\
            name instead of killed."
    )]
    Stop,
    #[command(
        about = "Stop, then start the server",
        long_about = "Full restart cycle: stop the current backend, re-validate\n\
            the active profile, and boot again with animated progress."
    )]
    Restart,
    #[command(
        about = "One-shot health + model probe",
        long_about = "Check the server process exists, then hit the provider's\n\
            health endpoint. Reports model id, speculation mode, and any\n\
            memory-pressure warnings the backend surfaces.",
        after_help = "EXAMPLES:\n    \
            modelsdesk health\n    \
            modelsdesk status --json"
    )]
    Health,
    #[command(
        visible_alias = "status",
        about = "Alias for health",
        hide = true
    )]
    Check,
    #[command(
        about = "List downloaded models and registry fit",
        long_about = "Scan the HuggingFace cache for weights with\n\
            real sizes, mark the active profile, and (when the server runs)\n\
            show the provider registry with per-model fit for this machine.",
        after_help = "EXAMPLES:\n    \
            modelsdesk models\n    \
            modelsdesk models --json | jq '.local[].repo'"
    )]
    Models,
    #[command(
        about = "Switch the active model",
        long_about = "Set the active profile. Same-backend switches hot-swap via\n\
            the provider's admin API (no restart); a provider change or\n\
            --restart does a full stop + animated boot. Unknown names are\n\
            registered as new profiles (provider guessed or via --provider).",
        after_help = "EXAMPLES:\n    \
            modelsdesk use lfm-2.5-8b\n    \
            modelsdesk use LiquidAI/LFM2.5-2.6B-MLX-bf16\n    \
            modelsdesk use qwen3-8b --provider llama-cpp\n    \
            modelsdesk use qwen3-8b --restart"
    )]
    Use {
        #[arg(help = "Profile name or repo id (see `modelsdesk models`)")]
        name: String,
        #[arg(long, help = "Force a full restart instead of hot-swap")]
        restart: bool,
        #[arg(
            long,
            help = "Provider backend for this model",
            long_help = "Pin the backend: mlx-dspark, mlx-lm, llama-cpp, ollama, strata,\n\
                mistral-rs, vllm, sglang. Without it, GGUF paths default to\n\
                llama-cpp, everything else\n\
                to mlx-dspark. Stored on the profile for next time."
        )]
        provider: Option<String>,
    },
    #[command(
        about = "List provider backends and install status",
        long_about = "Show every backend modelsdesk knows, whether its binary was\n\
            found (and where), which platforms it supports, and the install\n\
            command when missing.",
        after_help = "EXAMPLES:\n    \
            modelsdesk providers\n    \
            modelsdesk providers --json"
    )]
    Providers,
    #[command(
        about = "Download a model's weights up front",
        long_about = "Pre-fetch weights so `start` boots fast. Shows the RAM\n\
            estimate and refuses incompatible profiles before downloading\n\
            a byte. Without this, first `start` downloads silently inside\n\
            the boot animation.",
        after_help = "EXAMPLES:\n    \
            modelsdesk pull\n    \
            modelsdesk pull qwen3-8b"
    )]
    Pull {
        #[arg(help = "Profile name to fetch (default: active profile)")]
        name: Option<String>,
    },
    #[command(
        about = "Search HuggingFace for runnable models",
        long_about = "Query huggingface.co for text-generation models, then annotate\n\
            each hit: detected weight format, recommended backend, whether the\n\
            backend is installed, RAM estimate vs this machine, and whether\n\
            the weights are already cached. Sorted by downloads.",
        after_help = "EXAMPLES:\n    \
            modelsdesk search qwen3 8b\n    \
            modelsdesk search LiquidAI LFM --limit 20\n    \
            modelsdesk search mistral --json | jq '.[].repo'"
    )]
    Search {
        #[arg(help = "Search terms")]
        query: Vec<String>,
        #[arg(long, default_value_t = 15, help = "Max results (default 15)")]
        limit: usize,
    },
    #[command(
        about = "Set up a model end-to-end (find → validate → download → profile)",
        long_about = "One command from repo id to ready-to-serve: verify the repo\n\
            exists on HuggingFace, recommend (or take) the backend, run the\n\
            full doctor validation, download weights (ollama pull, or first\n\
            start for the rest), save the profile, and optionally start\n\
            serving immediately.\n\
            Missing backends auto-install unless --no-install.",
        after_help = "EXAMPLES:\n    \
            modelsdesk setup mlx-community/Qwen3-8B-8bit\n    \
            modelsdesk setup mlx-community/Qwen3-8B-8bit --name qwen3-8b --start\n    \
            modelsdesk setup ./models/mistral-7b.Q4_K_M.gguf --provider llama-cpp --start"
    )]
    Setup {
        #[arg(help = "Repo id, GGUF path, or ollama name")]
        repo: String,
        #[arg(long, help = "Profile short name (default: last repo segment)")]
        name: Option<String>,
        #[arg(long, help = "Provider backend (default: recommended for format)")]
        provider: Option<String>,
        #[arg(long, help = "DSpark/EAGLE drafter repo (mlx-dspark only)")]
        drafter: Option<String>,
        #[arg(long, help = "Start serving immediately after setup")]
        start: bool,
        #[arg(
            long,
            help = "Don't auto-install a missing backend (fail with the install command instead)"
        )]
        no_install: bool,
    },
    #[command(
        about = "Sync OpenCode/jcode configs to the active model",
        long_about = "Rewrite the local-model entries in OpenCode's opencode.json\n\
            and jcode's config.toml to point at modelsdesk's active profile\n\
            (same port, current model id + context window). Run after every\n\
            `use`/`setup` so switching models never breaks the agents.",
        after_help = "EXAMPLES:\n    \
            modelsdesk sync-agents\n    \
            modelsdesk use qwen3-8b && modelsdesk sync-agents"
    )]
    SyncAgents,
    #[command(
        about = "Show outdated backends and stale cached models",
        long_about = "Check every installed backend against its newest release\n\
            (PyPI / brew livecheck / GitHub releases) and every cached model\n\
            against its HuggingFace lastModified. Network-dependent; skips\n\
            quietly unreachable sources as 'unknown' instead of failing.",
        after_help = "EXAMPLES:\n    \
            modelsdesk outdated\n    \
            modelsdesk outdated --json"
    )]
    Outdated {
        #[arg(
            long,
            hide = true,
            help = "Refresh the ambient warning cache and exit silently"
        )]
        refresh_cache: bool,
    },
    #[command(
        about = "Validate the active model against provider + machine",
        long_about = "Pre-flight check without starting anything: provider\n\
            installed, weight format compatible, architecture supported\n\
            (MLX = Apple Silicon + macOS), RAM estimate vs this machine,\n\
            port availability. Fails fast with an actionable hint.",
        after_help = "EXAMPLES:\n    \
            modelsdesk doctor\n    \
            modelsdesk doctor qwen3-8b\n    \
            modelsdesk doctor --fix\n    \
            modelsdesk doctor --json"
    )]
    Doctor {
        #[arg(help = "Profile name to check (default: active profile)")]
        name: Option<String>,
        #[arg(
            long,
            help = "Install the missing backend automatically when not found"
        )]
        fix: bool,
    },
    #[command(
        about = "Manage stored model profiles",
        long_about = "Profiles are named model configurations in config.toml:\n\
            repo + provider + drafter + tuning (kv-bits, context window,\n\
            temperature, speculation mode). Add once, `use` forever.",
        after_help = "EXAMPLES:\n    \
            modelsdesk profile list\n    \
            modelsdesk profile add lfm-2.5-8b LiquidAI/LFM2.5-8B-A1B-MLX-4bit --drafter LiquidAI/LFM2.5-8B-A1B-DSpark\n    \
            modelsdesk profile show lfm-2.5-8b"
    )]
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },
    #[command(
        about = "Show the server log",
        after_help = "EXAMPLES:\n    \
            modelsdesk logs\n    \
            modelsdesk logs 100 --follow"
    )]
    Logs {
        #[arg(help = "Lines to show (default 30)")]
        lines: Option<usize>,
        #[arg(long, short, help = "Follow the log (Ctrl-C to stop)")]
        follow: bool,
    },
    #[command(
        about = "Animated live status console (q to quit)",
        long_about = "Full-screen console: server state, loaded model, health,\n\
            and a live log tail. Refreshes twice a second. Press q or Esc\n\
            to exit (the server keeps running)."
    )]
    Watch,
    #[command(
        about = "Write the active server into a coding agent's config",
        long_about = "Point a coding agent at the local modelsdesk server.\n\
            Writes an OpenAI-compatible provider entry for the active\n\
            profile and sets it as the agent's default model. Idempotent:\n\
            re-running updates the entry, other config is left alone.",
        after_help = "EXAMPLES:\n    \
            modelsdesk connect jcode\n    \
            modelsdesk connect codex\n    \
            modelsdesk connect opencode\n    \
            modelsdesk connect --list"
    )]
    Connect {
        #[arg(help = "Agent: jcode, codex, opencode, zed")]
        agent: Option<String>,
        #[arg(long, help = "List supported agents and their config paths")]
        list: bool,
        #[arg(long, help = "Show what would be written, don't touch files")]
        dry_run: bool,
    },
    #[command(
        about = "Print shell completions",
        after_help = "EXAMPLES:\n    \
            modelsdesk completions zsh > ~/.zsh/completions/_modelsdesk"
    )]
    Completions {
        #[arg(help = "Shell")]
        shell: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProfileCommands {
    #[command(about = "List stored profiles")]
    List,
    #[command(
        about = "Show one profile in detail",
        after_help = "EXAMPLES:\n    modelsdesk profile show lfm-2.5-8b"
    )]
    Show {
        #[arg(help = "Profile name")]
        name: String,
    },
    #[command(
        about = "Add a profile (validates before saving)",
        long_about = "Register a named model configuration. Format/provider/arch\n\
            compatibility is validated before anything is written — a bad\n\
            profile is rejected with the reason, not saved.",
        after_help = "EXAMPLES:\n    \
            modelsdesk profile add lfm-2.5-8b LiquidAI/LFM2.5-8B-A1B-MLX-4bit --drafter LiquidAI/LFM2.5-8B-A1B-DSpark\n    \
            modelsdesk profile add qwen3-8b mlx-community/Qwen3-8B-8bit --provider mlx-dspark\n    \
            modelsdesk profile add mistral-7b ./models/mistral-7b.Q4_K_M.gguf --provider llama-cpp --context-window 8192"
    )]
    Add {
        #[arg(help = "Short name, e.g. lfm-2.5-8b")]
        name: String,
        #[arg(help = "Repo id, GGUF path, or ollama name")]
        repo: String,
        #[arg(long, help = "Provider backend (default: guessed from repo)")]
        provider: Option<String>,
        #[arg(long, help = "DSpark/EAGLE drafter repo (mlx-dspark only)")]
        drafter: Option<String>,
        #[arg(
            long,
            default_value_t = 4,
            help = "KV-cache bits: 0, 4, or 8 (mlx-dspark)"
        )]
        kv_bits: u8,
        #[arg(long, help = "Cap context window (keeps KV cache inside RAM)")]
        context_window: Option<u32>,
        #[arg(long, help = "Default sampling temperature")]
        temperature: Option<f32>,
        #[arg(
            long,
            help = "Speculation mode: auto, dspark, baseline, lookup"
        )]
        mode: Option<String>,
        #[arg(long, help = "Replace an existing profile with this name")]
        overwrite: bool,
    },
    #[command(
        about = "Remove a profile",
        after_help = "EXAMPLES:\n    modelsdesk profile remove old-model"
    )]
    Remove {
        #[arg(help = "Profile name")]
        name: String,
    },
}
