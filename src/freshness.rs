use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::providers;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendFreshness {
    pub backend: String,
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub status: Freshness,
    pub update_cmd: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Current,
    UpdateAvailable,
    BelowMinimum,
    Unknown,
    NotInstalled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFreshness {
    pub repo: String,
    pub cached_snapshot: Option<String>,
    pub upstream_modified: Option<String>,
    pub status: Freshness,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AmbientReport {
    pub checked_at: i64,
    pub backends: Vec<BackendFreshness>,
    pub models: Vec<ModelFreshness>,
}

impl AmbientReport {
    pub fn has_warnings(&self) -> bool {
        let actionable = |s: Freshness| {
            matches!(s, Freshness::UpdateAvailable | Freshness::BelowMinimum)
        };
        self.backends.iter().any(|b| actionable(b.status))
            || self.models.iter().any(|m| actionable(m.status))
    }
}

const CACHE_TTL_SECS: i64 = 6 * 3600;

fn cache_path() -> std::path::PathBuf {
    crate::platform::data_dir().join("freshness.json")
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn read_cache() -> Option<AmbientReport> {
    let raw = std::fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn cache_fresh() -> bool {
    read_cache()
        .map(|r| now_secs() - r.checked_at < CACHE_TTL_SECS)
        .unwrap_or(false)
}

fn write_cache(report: &AmbientReport) {
    if let Some(parent) = cache_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string(report) {
        let _ = std::fs::write(cache_path(), raw);
    }
}

/// Full live check of backends + cached models. Network-bound; callers that
/// must stay fast should gate on `cache_fresh()` first.
pub async fn full_check(cfg: &crate::config::Config) -> AmbientReport {
    let mut backends = Vec::new();
    for p in providers::all() {
        backends.push(check_backend(&*p).await);
    }
    let mut models = Vec::new();
    for m in crate::models::local_models(cfg) {
        models.push(check_model(&m.repo).await);
    }
    let report = AmbientReport {
        checked_at: now_secs(),
        backends,
        models,
    };
    write_cache(&report);
    report
}

/// Print cached warnings to stderr (if any). Refreshes the cache in the
/// background when stale — never blocks the calling command.
pub fn maybe_warn(cfg: &crate::config::Config) {
    let cached = read_cache();
    let stale = cached
        .as_ref()
        .map(|r| now_secs() - r.checked_at >= CACHE_TTL_SECS)
        .unwrap_or(true);
    if let Some(report) = cached {
        print_warnings(&report);
    }
    if stale {
        refresh_in_background();
    }
    let _ = cfg;
}

fn print_warnings(report: &AmbientReport) {
    let mut lines = Vec::new();
    for b in &report.backends {
        match b.status {
            Freshness::UpdateAvailable => lines.push(format!(
                "{} {} v{} → v{} ({})",
                b.backend,
                "update available:",
                b.installed.as_deref().unwrap_or("?"),
                b.latest.as_deref().unwrap_or("?"),
                b.update_cmd.as_deref().unwrap_or("modelsdesk outdated")
            )),
            Freshness::BelowMinimum => lines.push(format!(
                "{} {} v{} is below minimum — upgrade now ({})",
                b.backend,
                "OUTDATED:",
                b.installed.as_deref().unwrap_or("?"),
                b.update_cmd.as_deref().unwrap_or("modelsdesk outdated")
            )),
            _ => {}
        }
    }
    for m in &report.models {
        if m.status == Freshness::UpdateAvailable {
            lines.push(format!(
                "{} upstream has newer weights (upstream {}) — re-pull to refresh",
                m.repo,
                m.upstream_modified.as_deref().unwrap_or("?")
            ));
        }
    }
    if lines.is_empty() {
        return;
    }
    eprintln!(
        "{} {} — run `modelsdesk outdated` for details",
        crate::output::warn("outdated:"),
        lines.join("; ")
    );
}

fn refresh_in_background() {
    // Detached child re-runs the check and rewrites the cache. Best-effort:
    // failures are silent by design (the next command retries).
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "modelsdesk".to_string());
    let _ = std::process::Command::new(exe)
        .args(["outdated", "--refresh-cache"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("modelsdesk/0.1.0")
        .build()
        .expect("freshness http client")
}

async fn pypi_latest(pkg: &str) -> Result<Option<String>> {
    let resp = http()
        .get(format!("https://pypi.org/pypi/{pkg}/json"))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let v: serde_json::Value = resp.json().await?;
    Ok(v
        .get("info")
        .and_then(|i| i.get("version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}

async fn brew_livecheck(formula: &str) -> Result<Option<String>> {
    // `brew livecheck --json` reports the newest upstream version known to the tap.
    let out = tokio::process::Command::new("brew")
        .args(["livecheck", "--json", formula])
        .output()
        .await?;
    if !out.status.success() {
        return Ok(None);
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    Ok(v
        .as_array()
        .and_then(|a| a.first())
        .and_then(|e| e.get("version"))
        .and_then(|x| x.get("latest"))
        .and_then(|l| l.as_str())
        .map(|s| s.to_string()))
}

async fn github_latest(owner_repo: &str) -> Result<Option<String>> {
    let resp = http()
        .get(format!(
            "https://api.github.com/repos/{owner_repo}/releases/latest"
        ))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let v: serde_json::Value = resp.json().await?;
    Ok(v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .and_then(|t| providers::extract_version(t))
        .or_else(|| {
            v.get("name")
                .and_then(|t| t.as_str())
                .and_then(|t| providers::extract_version(t))
        }))
}

pub async fn backend_latest(backend: &str) -> Option<String> {
    match backend {
        "mlx-dspark" => pypi_latest("mlx-dspark").await.ok()?,
        "mlx-lm" => {
            if let Some(v) = brew_livecheck("mlx-lm").await.ok()? {
                return Some(v);
            }
            pypi_latest("mlx-lm").await.ok()?
        }
        "llama-cpp" => github_latest("ggml-org/llama.cpp").await.ok()?,
        "ollama" => {
            if let Some(v) = brew_livecheck("ollama").await.ok()? {
                return Some(v);
            }
            github_latest("ollama/ollama").await.ok()?
        }
        "strata" => github_latest("Niko1221/Strata").await.ok()?,
        "mistral-rs" => github_latest("EricLBuehler/mistral.rs").await.ok()?,
        "vllm" => github_latest("vllm-project/vllm").await.ok()?,
        "sglang" => github_latest("sgl-project/sglang").await.ok()?,
        _ => None,
    }
}

pub fn update_command(backend: &str) -> Option<String> {
    match backend {
        "mlx-dspark" => Some("pipx upgrade mlx-dspark".to_string()),
        "mlx-lm" => Some("brew upgrade mlx-lm".to_string()),
        "llama-cpp" => Some("brew upgrade llama.cpp".to_string()),
        "ollama" => Some("brew upgrade ollama".to_string()),
        "strata" => Some("strata update".to_string()),
        "mistral-rs" => Some("curl -fsSL https://mistralrs.dev/install.sh | sh # re-run to update".to_string()),
        "vllm" => Some("pip install -U vllm".to_string()),
        "sglang" => Some("pip install -U sglang".to_string()),
        _ => None,
    }
}

pub async fn check_backend(p: &dyn providers::Provider) -> BackendFreshness {
    let installed = p.installed_version().unwrap_or(None);
    let latest = backend_latest(p.id()).await;
    let status = match (&installed, &latest) {
        (None, _) => Freshness::NotInstalled,
        (Some(_), None) => {
            if let Some(min) = p.min_version() {
                if !providers::version_meets(installed.as_deref().unwrap_or(""), min) {
                    Freshness::BelowMinimum
                } else {
                    Freshness::Unknown
                }
            } else {
                Freshness::Unknown
            }
        }
        (Some(cur), Some(lat)) => {
            if let Some(min) = p.min_version() {
                if !providers::version_meets(cur, min) {
                    Freshness::BelowMinimum
                } else if providers::version_meets(lat, parse(cur))
                    && lat != cur
                {
                    Freshness::UpdateAvailable
                } else if lat == cur {
                    Freshness::Current
                } else {
                    Freshness::UpdateAvailable
                }
            } else if lat == cur {
                Freshness::Current
            } else {
                Freshness::UpdateAvailable
            }
        }
    };
    BackendFreshness {
        backend: p.id().to_string(),
        installed,
        latest,
        status,
        update_cmd: update_command(p.id()),
    }
}

fn parse(v: &str) -> (u64, u64, u64) {
    let mut it = v.split('.');
    (
        it.next().and_then(|p| p.parse().ok()).unwrap_or(0),
        it.next().and_then(|p| p.parse().ok()).unwrap_or(0),
        it.next().and_then(|p| p.parse().ok()).unwrap_or(0),
    )
}

pub async fn check_model(repo: &str) -> ModelFreshness {
    let cached = cached_snapshot_info(repo);
    let upstream = upstream_modified(repo).await.unwrap_or(None);
    let status = match (&cached, &upstream) {
        (None, _) => Freshness::NotInstalled,
        (Some(_), None) => Freshness::Unknown,
        (Some(c), Some(u)) => {
            if !u.newer_than(c.downloaded_at) {
                Freshness::Current
            } else {
                Freshness::UpdateAvailable
            }
        }
    };
    ModelFreshness {
        repo: repo.to_string(),
        cached_snapshot: cached.map(|c| format!("{} (cached {})", c.commit, humantime(c.downloaded_at))),
        upstream_modified: upstream.map(|u| u.raw),
        status,
    }
}

struct CachedInfo {
    commit: String,
    downloaded_at: std::time::SystemTime,
}

struct UpstreamInfo {
    raw: String,
    timestamp: i64,
}

impl UpstreamInfo {
    fn newer_than(&self, t: std::time::SystemTime) -> bool {
        let upstream_secs = self.timestamp;
        let cached_secs = t
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        // 24h grace: HF lastModified bumps on metadata edits, not new weights.
        upstream_secs > cached_secs + 86400
    }
}

fn cached_snapshot_info(repo: &str) -> Option<CachedInfo> {
    let slug = format!("models--{}", repo.replace('/', "--"));
    let dir = crate::platform::hf_hub_cache().join(&slug).join("snapshots");
    let (mtime, name) = std::fs::read_dir(&dir).ok()?.flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        let mtime = e.metadata().ok()?.modified().ok()?;
        Some((mtime, name))
    }).max_by_key(|(t, _)| *t)?;
    Some(CachedInfo {
        commit: name.chars().take(8).collect(),
        downloaded_at: mtime,
    })
}

async fn upstream_modified(repo: &str) -> Result<Option<UpstreamInfo>, ()> {
    let resp = http()
        .get(format!("https://huggingface.co/api/models/{repo}"))
        .send()
        .await
        .map_err(|_| ())?;
    if !resp.status().is_success() {
        return Err(());
    }
    let v: serde_json::Value = resp.json().await.map_err(|_| ())?;
    let raw = v
        .get("lastModified")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());
    Ok(raw.map(|r| UpstreamInfo {
        timestamp: parse_iso8601(&r).unwrap_or(0),
        raw: r,
    }))
}

/// Parse "2026-09-14T10:23:45.000Z" (or without millis) to unix seconds.
fn parse_iso8601(s: &str) -> Option<i64> {
    let s = s.trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, mo, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let time = time.split('.').next()?;
    let mut t = time.split(':');
    let (h, mi, sec): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    // Days since epoch (Howard Hinnant's civil_from_days, inverted).
    let y_adj = if mo <= 2 { y - 1 } else { y };
    let era = y_adj.div_euclid(400);
    let yoe = y_adj - era * 400;
    let mp = ((mo + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + sec)
}

fn humantime(_t: std::time::SystemTime) -> String {
    match _t.elapsed() {
        Ok(d) => {
            let days = d.as_secs() / 86400;
            if days == 0 {
                "today".to_string()
            } else if days == 1 {
                "yesterday".to_string()
            } else {
                format!("{days}d ago")
            }
        }
        Err(_) => "recently".to_string(),
    }
}
