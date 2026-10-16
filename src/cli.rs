use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "modeldesk",
    version,
    about = "Manage local LLM servers — start/stop, health, models, providers",
    long_about = "ModelDesk manages local LLM inference servers on your machine.\n\
        \n\
        macOS first (MLX + mlx-dspark), provider-agnostic core: llama.cpp and\n\
        Ollama work wherever their binaries run, so Linux/Windows follow.\n\
        \n\
        QUICK START:\n    \
        modeldesk providers                 # what backends are installed\n    \
        modeldesk models                    # what weights are downloaded\n    \
        modeldesk start                     # launch the active model\n    \
        modeldesk health                    # is it answering?\n    \
        modeldesk watch                     # live animated console\n\
        \n\
        Config lives at <config-dir>/modeldesk/config.toml — edit it directly\n\
        or use `modeldesk profile add`. Logs at <data-dir>/modeldesk/logs/.",
    after_help = "EXAMPLES:\n    \
        modeldesk start --foreground\n    \
        modeldesk use qwen3-8b --provider llama-cpp\n    \
        modeldesk profile add lfm-2.5-8b LiquidAI/LFM2.5-8B-A1B-MLX-4bit --drafter LiquidAI/LFM2.5-8B-A1B-DSpark\n    \
        modeldesk doctor\n    \
        modeldesk health --json | jq .",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
    #[arg(
        long,
        global = true,
        help = "Emit machine-readable JSON where supported",
        long_help = "Emit machine-readable JSON instead of human output.\nSupported by: health, models, providers, use, doctor, search, setup."
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
            compatible, enough RAM, port free), then launch the backend with\n\
            animated progress. First run downloads weights — expect minutes.",
        after_help = "EXAMPLES:\n    \
            modeldesk start\n    \
            modeldesk start --foreground    # attach; Ctrl-C stops the server"
    )]
    Start {
        #[arg(long, help = "Run in foreground (attach to server output)")]
        foreground: bool,
    },
    #[command(
        about = "Stop the running model server",
        long_about = "Stop the server modeldesk started. Only ever kills its own\n\
            backend process on the configured port — never a foreign process\n\
            (if another tool holds the port, it is left alone and reported)."
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
            modeldesk health\n    \
            modeldesk status --json"
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
        long_about = "Scan the HuggingFace + LM Studio caches for weights with\n\
            real sizes, mark the active profile, and (when the server runs)\n\
            show the provider registry with per-model fit for this machine.",
        after_help = "EXAMPLES:\n    \
            modeldesk models\n    \
            modeldesk models --json | jq '.local[].repo'"
    )]
    Models,
    #[command(
        about = "Switch the active model",
        long_about = "Set the active profile. Same-backend switches hot-swap via\n\
            the provider's admin API (no restart); a provider change or\n\
            --restart does a full stop + animated boot. Unknown names are\n\
            registered as new profiles (provider guessed or via --provider).",
        after_help = "EXAMPLES:\n    \
            modeldesk use lfm-2.5-8b\n    \
            modeldesk use LiquidAI/LFM2.5-2.6B-MLX-bf16\n    \
            modeldesk use qwen3-8b --provider llama-cpp\n    \
            modeldesk use qwen3-8b --restart"
    )]
    Use {
        #[arg(help = "Profile name or repo id (see `modeldesk models`)")]
        name: String,
        #[arg(long, help = "Force a full restart instead of hot-swap")]
        restart: bool,
        #[arg(
            long,
            help = "Provider backend for this model",
            long_help = "Pin the backend: mlx-dspark, mlx-lm, llama-cpp, ollama.\n\
                Without it, GGUF paths default to llama-cpp, everything else\n\
                to mlx-dspark. Stored on the profile for next time."
        )]
        provider: Option<String>,
    },
    #[command(
        about = "List provider backends and install status",
        long_about = "Show every backend modeldesk knows, whether its binary was\n\
            found (and where), which platforms it supports, and the install\n\
            command when missing.",
        after_help = "EXAMPLES:\n    \
            modeldesk providers\n    \
            modeldesk providers --json"
    )]
    Providers,
    #[command(
        about = "Download a model's weights up front",
        long_about = "Pre-fetch weights so `start` boots fast. Shows the RAM\n\
            estimate and refuses incompatible profiles before downloading\n\
            a byte. Without this, first `start` downloads silently inside\n\
            the boot animation.",
        after_help = "EXAMPLES:\n    \
            modeldesk pull\n    \
            modeldesk pull qwen3-8b"
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
            modeldesk search qwen3 8b\n    \
            modeldesk search LiquidAI LFM --limit 20\n    \
            modeldesk search mistral --json | jq '.[].repo'"
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
            serving immediately.",
        after_help = "EXAMPLES:\n    \
            modeldesk setup mlx-community/Qwen3-8B-8bit\n    \
            modeldesk setup mlx-community/Qwen3-8B-8bit --name qwen3-8b --start\n    \
            modeldesk setup ./models/mistral-7b.Q4_K_M.gguf --provider llama-cpp --start"
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
    },
    #[command(
        about = "Validate the active model against provider + machine",
        long_about = "Pre-flight check without starting anything: provider\n\
            installed, weight format compatible, architecture supported\n\
            (MLX = Apple Silicon + macOS), RAM estimate vs this machine,\n\
            port availability. Fails fast with an actionable hint.",
        after_help = "EXAMPLES:\n    \
            modeldesk doctor\n    \
            modeldesk doctor qwen3-8b\n    \
            modeldesk doctor --json"
    )]
    Doctor {
        #[arg(help = "Profile name to check (default: active profile)")]
        name: Option<String>,
    },
    #[command(
        about = "Manage stored model profiles",
        long_about = "Profiles are named model configurations in config.toml:\n\
            repo + provider + drafter + tuning (kv-bits, context window,\n\
            temperature, speculation mode). Add once, `use` forever.",
        after_help = "EXAMPLES:\n    \
            modeldesk profile list\n    \
            modeldesk profile add lfm-2.5-8b LiquidAI/LFM2.5-8B-A1B-MLX-4bit --drafter LiquidAI/LFM2.5-8B-A1B-DSpark\n    \
            modeldesk profile show lfm-2.5-8b"
    )]
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },
    #[command(
        about = "Show the server log",
        after_help = "EXAMPLES:\n    \
            modeldesk logs\n    \
            modeldesk logs 100 --follow"
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
        about = "Print shell completions",
        after_help = "EXAMPLES:\n    \
            modeldesk completions zsh > ~/.zsh/completions/_modeldesk"
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
        after_help = "EXAMPLES:\n    modeldesk profile show lfm-2.5-8b"
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
            modeldesk profile add lfm-2.5-8b LiquidAI/LFM2.5-8B-A1B-MLX-4bit --drafter LiquidAI/LFM2.5-8B-A1B-DSpark\n    \
            modeldesk profile add qwen3-8b mlx-community/Qwen3-8B-8bit --provider mlx-dspark\n    \
            modeldesk profile add mistral-7b ./models/mistral-7b.Q4_K_M.gguf --provider llama-cpp --context-window 8192"
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
        after_help = "EXAMPLES:\n    modeldesk profile remove old-model"
    )]
    Remove {
        #[arg(help = "Profile name")]
        name: String,
    },
}
