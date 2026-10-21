use anyhow::Result;
use async_trait::async_trait;

use crate::config::ModelEntry;
use crate::health::HealthReport;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    Running { pid: u32, port: u16 },
    Stopped,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn display(&self) -> &'static str;
    fn start_args(&self, entry: &ModelEntry, host: &str, port: u16) -> Result<Vec<String>>;
    fn binary(&self) -> Result<std::path::PathBuf>;
    fn match_score(&self, entry: &ModelEntry) -> u8 {
        if entry.provider == self.id() {
            100
        } else {
            0
        }
    }
    async fn health(&self, client: &reqwest::Client, host: &str, port: u16)
        -> Result<HealthReport>;
    async fn admin_models(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
    ) -> Result<Option<serde_json::Value>>;
    async fn admin_load(
        &self,
        client: &reqwest::Client,
        host: &str,
        port: u16,
        entry: &ModelEntry,
    ) -> Result<bool>;

    /// CLI args that print the backend version (default: --version).
    fn version_args(&self) -> Vec<String> {
        vec!["--version".to_string()]
    }

    /// Minimum known-good version (major, minor, patch). None = unpinned.
    fn min_version(&self) -> Option<(u64, u64, u64)> {
        None
    }

    /// Download weights for this entry. Returns the local path serving should
    /// use. Default: backends that self-fetch on first start (MLX) do nothing
    /// and return None (caller keeps the repo id).
    async fn fetch_weights(&self, _entry: &ModelEntry) -> Result<Option<std::path::PathBuf>> {
        Ok(None)
    }

    fn installed_version(&self) -> Result<Option<String>> {
        let bin = match self.binary() {
            Ok(b) => b,
            Err(_) => return Ok(None),
        };
        // 1. Backend's own --version (or override).
        if let Ok(out) = std::process::Command::new(&bin)
            .args(self.version_args())
            .output()
        {
            if out.status.success() {
                let text = format!(
                    "{} {}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                if let Some(v) = extract_version(&text) {
                    return Ok(Some(v));
                }
            }
        }
        // 2. Package-manager records (backends without --version).
        Ok(package_version(self.id()))
    }
}

/// First MAJOR.MINOR.PATCH-ish token in free-form version output.
pub fn extract_version(text: &str) -> Option<String> {
    let mut buf = String::new();
    let mut dots = 0;
    let mut started = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            started = true;
            buf.push(ch);
        } else if ch == '.' && started && dots < 2 {
            dots += 1;
            buf.push(ch);
        } else if started {
            if buf.chars().any(|c| c.is_ascii_digit()) && dots >= 1 {
                return Some(buf.trim_end_matches('.').to_string());
            }
            buf.clear();
            dots = 0;
            started = false;
        }
    }
    if started && dots >= 1 {
        return Some(buf.trim_end_matches('.').to_string());
    }
    None
}

pub fn version_meets(version: &str, min: (u64, u64, u64)) -> bool {
    let parse = |v: &str| {
        let mut it = v.split('.');
        (
            it.next().and_then(|p| p.parse().ok()).unwrap_or(0),
            it.next().and_then(|p| p.parse().ok()).unwrap_or(0),
            it.next().and_then(|p| p.parse().ok()).unwrap_or(0),
        )
    };
    parse(version) >= min
}

/// Version from the installing package manager, for backends whose binary
/// has no --version (mlx-dspark via pipx, mlx-lm via brew).
fn package_version(backend: &str) -> Option<String> {
    match backend {
        "mlx-dspark" => {
            pipx_version("mlx-dspark").or_else(|| pip_version("mlx-dspark"))
        }
        "mlx-lm" => brew_version("mlx-lm"),
        "llama-cpp" => brew_version("llama.cpp"),
        "ollama" => brew_version("ollama"),
        "strata" => None, // installs via its own install.sh, no package-manager record
        "mistral-rs" => None,
        "vllm" => pip_version_pub("vllm"),
        "sglang" => pip_version_pub("sglang"),
        "tabby" => None, // git-clone install; no package-manager record
        _ => None,
    }
}

fn cmd_out(prog: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(prog)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
}

fn pipx_version(pkg: &str) -> Option<String> {
    let out = cmd_out("pipx", &["list", "--short"])?;
    out.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let name = parts.next()?;
        if name.eq_ignore_ascii_case(pkg) {
            let ver = parts.next()?.trim_matches(|c| c == '(' || c == ')' || c == ',');
            extract_version(ver)
        } else {
            None
        }
    })
}

fn pip_version(pkg: &str) -> Option<String> {
    let out = cmd_out("pipx", &["runpip", pkg, "show", pkg]).or_else(|| {
        cmd_out(
            "python3",
            &["-m", "pip", "show", pkg],
        )
    })?;
    out.lines().find_map(|l| {
        l.strip_prefix("Version:")
            .map(|v| v.trim().to_string())
            .and_then(|v| extract_version(&v))
    })
}

/// Public wrapper for package_version lookups (vllm, sglang via pip).
fn pip_version_pub(pkg: &str) -> Option<String> {
    pip_version(pkg)
}

fn brew_version(formula: &str) -> Option<String> {
    let out = cmd_out("brew", &["list", "--versions", formula])?;
    // "mlx-lm 0.32.0_1" → take the first version-looking token.
    out.split_whitespace()
        .skip(1)
        .find_map(|tok| extract_version(tok.trim_end_matches(",") ))
}

pub mod llama_cpp;
pub mod mistral_rs;
pub mod mlx_dspark;
pub mod mlx_lm;
pub mod vllm;
pub mod ollama;
pub mod sglang;
pub mod strata;
pub mod tabby;

pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(mlx_dspark::MlxDspark),
        Box::new(mlx_lm::MlxLm),
        Box::new(llama_cpp::LlamaCpp),
        Box::new(ollama::Ollama),
        Box::new(strata::Strata),
        Box::new(mistral_rs::MistralRs),
        Box::new(vllm::Vllm),
        Box::new(sglang::Sglang),
        Box::new(tabby::Tabby),
    ]
}

pub fn for_entry(entry: &ModelEntry) -> Box<dyn Provider> {
    let mut ranked: Vec<(u8, Box<dyn Provider>)> = all()
        .into_iter()
        .map(|p| {
            let s = p.match_score(entry);
            (s, p)
        })
        .collect();
    ranked.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    ranked.into_iter().next().map(|(_, p)| p).unwrap()
}
