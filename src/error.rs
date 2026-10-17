use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModeldeskError {
    #[error("provider '{provider}' is not installed: {hint}")]
    ProviderMissing { provider: String, hint: String },

    #[error("model '{repo}' is incompatible with provider '{provider}': {reason}")]
    Incompatible {
        repo: String,
        provider: String,
        reason: String,
    },

    #[error("model '{repo}' needs ~{need_gb:.1} GB but this machine has {have_gb:.1} GB usable")]
    InsufficientMemory {
        repo: String,
        need_gb: f64,
        have_gb: f64,
    },

    #[error(
        "port :{port} is held by another process ({holder}) — pick a free port or stop it first"
    )]
    PortBusy { port: u16, holder: String },

    #[error("server on :{port} did not become healthy in {timeout_s}s (last: {last})")]
    Unhealthy {
        port: u16,
        timeout_s: u64,
        last: String,
    },

    #[error("no model named '{name}' — see `modelsdesk models`")]
    UnknownModel { name: String },

    #[error("unknown provider '{id}' — see `modelsdesk providers`")]
    UnknownProvider { id: String },

    #[error("profile '{name}' already exists (use --overwrite to replace it)")]
    ProfileExists { name: String },

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl ModeldeskError {
    pub fn hint(&self) -> Option<String> {
        match self {
            Self::ProviderMissing { provider, .. } => Some(match provider.as_str() {
                "mlx-dspark" => "pipx install mlx-dspark".to_string(),
                "mlx-lm" => "brew install mlx-lm".to_string(),
                "llama-cpp" => "brew install llama.cpp".to_string(),
                "ollama" => "brew install ollama".to_string(),
                _ => format!("install the '{provider}' backend"),
            }),
            Self::Incompatible { provider, .. } => Some(
                format!("try `modelsdesk use <model> --provider <id>` — see `modelsdesk providers`")
                    .replace("<id>", provider),
            ),
            Self::InsufficientMemory { .. } => {
                Some("use a smaller quant (4-bit) or a smaller model".to_string())
            }
            Self::PortBusy { .. } => {
                Some("`lsof -i :<port>` shows the holder; never kill it blindly".to_string())
            }
            Self::Unhealthy { .. } => Some("`modelsdesk logs` shows why it failed".to_string()),
            _ => None,
        }
    }
}
