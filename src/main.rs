use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::BufRead;
use std::time::Duration;

use modeldesk::cli::{Cli, Commands, ProfileCommands};
use modeldesk::compat;
use modeldesk::config::{Config, ModelEntry};
use modeldesk::error::ModeldeskError;
use modeldesk::providers::ServerStatus;
use modeldesk::{models, output, platform, providers, server, watch};

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {e:#}", output::err("error"));
        let hint = hint_for(&e);
        if let Some(h) = hint {
            eprintln!("{} {h}", output::dim("hint:"));
        }
        std::process::exit(1);
    }
}

#[tokio::main]
async fn run() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(if cli.verbose {
            "debug"
        } else {
            "warn"
        }))
        .with_writer(std::io::stderr)
        .init();

    if !platform::supported_now() {
        eprintln!(
            "{}",
            output::warn(&format!(
                "modeldesk targets macOS first; running on {} (provider-agnostic core, YMMV)",
                platform::os_label()
            ))
        );
    }

    match cli.command {
        Commands::Start { foreground } => {
            let cfg = Config::load()?;
            let entry = cfg.active_entry()?;
            if foreground {
                server::start(&cfg, &entry, true).await?;
            } else {
                watch::animated_boot(&cfg).await?;
            }
        }
        Commands::Stop => {
            let cfg = Config::load()?;
            match server::server_status(&cfg) {
                ServerStatus::Stopped => println!("{}", output::dim("already stopped")),
                ServerStatus::Running { pid, port } => {
                    let pb = ProgressBar::new_spinner();
                    pb.set_style(ProgressStyle::with_template("{spinner:.red} {msg}")?);
                    pb.set_message(format!("stopping pid {pid} on :{port}"));
                    pb.enable_steady_tick(Duration::from_millis(80));
                    server::stop(&cfg).await?;
                    pb.finish_with_message(output::ok(&format!("port :{port} free")));
                }
            }
        }
        Commands::Restart => {
            let cfg = Config::load()?;
            if !matches!(server::server_status(&cfg), ServerStatus::Stopped) {
                server::stop(&cfg).await?;
            }
            watch::animated_boot(&cfg).await?;
        }
        Commands::Health | Commands::Check => {
            let cfg = Config::load()?;
            run_health(&cfg, cli.json).await?;
        }
        Commands::Models => {
            let cfg = Config::load()?;
            run_models(&cfg, cli.json).await?;
        }
        Commands::Use {
            name,
            restart,
            provider,
        } => {
            let mut cfg = Config::load()?;
            run_use(&mut cfg, &name, restart, provider.as_deref(), cli.json).await?;
        }
        Commands::Providers => {
            run_providers(cli.json);
        }
        Commands::Pull { name } => {
            let cfg = Config::load()?;
            run_pull(&cfg, name.as_deref(), cli.json)?;
        }
        Commands::Doctor { name } => {
            let cfg = Config::load()?;
            run_doctor(&cfg, name.as_deref(), cli.json)?;
        }
        Commands::Profile { command } => {
            let mut cfg = Config::load()?;
            run_profile(&mut cfg, command, cli.json)?;
        }
        Commands::Logs { lines, follow } => {
            let n = lines.unwrap_or(30);
            if follow {
                let path = platform::log_path();
                let file = std::fs::File::open(&path)
                    .with_context(|| format!("opening {}", path.display()))?;
                let mut reader = std::io::BufReader::new(file);
                let mut buf = String::new();
                loop {
                    buf.clear();
                    let n = reader.read_line(&mut buf)?;
                    if n == 0 {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        continue;
                    }
                    print!("{buf}");
                }
            } else {
                let tail = server::tail_log(n)?;
                print!("{tail}");
                if !tail.ends_with('\n') {
                    println!();
                }
            }
        }
        Commands::Watch => {
            let cfg = Config::load()?;
            watch::live_console(&cfg).await?;
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let shell = shell.to_lowercase();
            match shell.as_str() {
                "bash" => generate(
                    clap_complete::shells::Bash,
                    &mut cmd,
                    "modeldesk",
                    &mut std::io::stdout(),
                ),
                "zsh" => generate(
                    clap_complete::shells::Zsh,
                    &mut cmd,
                    "modeldesk",
                    &mut std::io::stdout(),
                ),
                "fish" => generate(
                    clap_complete::shells::Fish,
                    &mut cmd,
                    "modeldesk",
                    &mut std::io::stdout(),
                ),
                "powershell" => generate(
                    clap_complete::shells::PowerShell,
                    &mut cmd,
                    "modeldesk",
                    &mut std::io::stdout(),
                ),
                "elvish" => generate(
                    clap_complete::shells::Elvish,
                    &mut cmd,
                    "modeldesk",
                    &mut std::io::stdout(),
                ),
                _ => anyhow::bail!("unknown shell '{shell}' (bash, zsh, fish, powershell, elvish)"),
            }
        }
    }
    Ok(())
}

async fn run_health(cfg: &Config, json: bool) -> Result<()> {
    let client = server::http_client();
    let status = server::server_status(cfg);
    let (health, provider_id) = match &status {
        ServerStatus::Stopped => (None, None),
        ServerStatus::Running { .. } => {
            let entry = cfg.active_entry()?;
            let provider = providers::for_entry(&entry);
            let rep = provider.health(&client, &cfg.host, cfg.port).await.ok();
            (rep, Some(provider.id().to_string()))
        }
    };
    if json {
        let v = serde_json::json!({
            "port": cfg.port,
            "status": match &status {
                ServerStatus::Running { pid, .. } => format!("running:{pid}"),
                ServerStatus::Stopped => "stopped".to_string(),
            },
            "provider": provider_id,
            "health": health.as_ref().map(|h| serde_json::json!({
                "ok": h.ok, "status_line": h.status_line,
                "model": h.model, "mode": h.mode,
            })),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    match &status {
        ServerStatus::Stopped => println!("{} (port :{} free)", output::dim("○ stopped"), cfg.port),
        ServerStatus::Running { pid, port } => match health {
            Some(h) if h.ok => println!(
                "{} pid={pid} :{port} {} {}",
                output::ok("healthy"),
                h.model.as_deref().unwrap_or("?"),
                h.mode
                    .as_deref()
                    .map(|m| format!("({m})"))
                    .unwrap_or_default()
            ),
            Some(h) => println!(
                "{} {} — {}",
                output::warn("degraded"),
                h.status_line,
                h.detail.chars().take(300).collect::<String>()
            ),
            None => println!(
                "{} pid={pid} :{port} (no health response yet)",
                output::warn("starting")
            ),
        },
    }
    Ok(())
}

async fn run_models(cfg: &Config, json: bool) -> Result<()> {
    let local = models::local_models(cfg);
    let registry = models::registry_models(cfg).await.unwrap_or_default();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "active": cfg.active, "local": local, "registry": registry,
            }))?
        );
        return Ok(());
    }
    println!(
        "{}",
        output::bold(&format!(
            "active: {}",
            cfg.active.as_deref().unwrap_or("(none)")
        ))
    );
    println!("\n{}", output::bold("downloaded:"));
    if local.is_empty() {
        println!("  {}", output::dim("(none found in HF/LMStudio caches)"));
    }
    for m in &local {
        let size = m
            .size_bytes
            .map(models::format_bytes)
            .unwrap_or_else(|| "?".to_string());
        let mark = if m.active {
            output::cyan("*")
        } else {
            " ".to_string()
        };
        println!("  {mark} {} [{}] {}", m.repo, m.source, output::dim(&size));
    }
    if !registry.is_empty() {
        println!("\n{}", output::bold("registry (fits this Mac):"));
        for r in &registry {
            let fit = if r.fits {
                output::ok("fits")
            } else {
                output::err("too big")
            };
            let ready = if r.ready {
                output::dim("cached")
            } else {
                output::dim("download on use")
            };
            println!(
                "  {} {} {} {} {}",
                r.id,
                output::dim(&r.target),
                output::dim(&format!("{} {}", r.mode, r.ram)),
                fit,
                ready
            );
        }
    }
    Ok(())
}

async fn run_use(
    cfg: &mut Config,
    name: &str,
    force_restart: bool,
    provider_override: Option<&str>,
    json: bool,
) -> Result<()> {
    if let Some(want) = provider_override {
        let valid = providers::all().iter().any(|p| p.id() == want);
        if !valid {
            anyhow::bail!("unknown provider '{want}' (see `modeldesk providers`)",);
        }
    }
    let previous = cfg.active_entry().ok();
    if let Some(entry) = cfg.resolve(name) {
        cfg.active = Some(entry.name.clone());
        if let Some(want) = provider_override {
            if let Some(m) = cfg
                .models
                .iter_mut()
                .find(|m| Some(&m.name) == cfg.active.as_ref())
            {
                m.provider = want.to_string();
            }
        }
        cfg.save()?;
    } else {
        let repo = name.to_string();
        let provider = provider_override.map(|s| s.to_string()).unwrap_or_else(|| {
            if repo.to_lowercase().ends_with(".gguf") || repo.contains(".gguf") {
                "llama-cpp".to_string()
            } else {
                "mlx-dspark".to_string()
            }
        });
        let short = repo.split('/').next_back().unwrap_or(&repo).to_string();
        cfg.models.push(modeldesk::config::ModelEntry {
            name: short.clone(),
            provider: provider.to_string(),
            repo: repo.clone(),
            drafter: None,
            kv_bits: 4,
            context_window: None,
            default_temperature: None,
            mode: None,
        });
        cfg.active = Some(short);
        cfg.save()?;
    }
    let entry = cfg.active_entry()?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "active": entry.name, "provider": entry.provider, "repo": entry.repo,
            }))?
        );
    } else {
        println!(
            "{} active → {} [{}] ({})",
            output::cyan("●"),
            entry.name,
            entry.provider,
            output::dim(&entry.repo)
        );
    }

    let running = !matches!(server::server_status(cfg), ServerStatus::Stopped);
    if !running {
        println!(
            "{}",
            output::dim("server stopped — `modeldesk start` to load it")
        );
        return Ok(());
    }
    let switched_provider = previous
        .as_ref()
        .map(|p| p.provider != entry.provider)
        .unwrap_or(false);
    if force_restart || switched_provider {
        if switched_provider && !force_restart {
            println!(
                "{}",
                output::dim(&format!(
                    "provider changed ({} → {}) — hot-swap is same-backend only, restarting",
                    previous
                        .as_ref()
                        .map(|p| p.provider.as_str())
                        .unwrap_or("?"),
                    entry.provider
                ))
            );
        }
        server::stop(cfg).await?;
        watch::animated_boot(cfg).await?;
        return Ok(());
    }
    let client = server::http_client();
    let provider = providers::for_entry(&entry);
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::with_template("{spinner:.green} {msg}")?);
    pb.set_message(format!("hot-swapping to {}", entry.repo));
    pb.enable_steady_tick(Duration::from_millis(80));
    match provider
        .admin_load(&client, &cfg.host, cfg.port, &entry)
        .await
    {
        Ok(true) => pb.finish_with_message(output::ok(&format!("now serving {}", entry.repo))),
        _ => {
            pb.finish_with_message(output::warn("hot-swap unsupported — restarting"));
            server::stop(cfg).await?;
            watch::animated_boot(cfg).await?;
        }
    }
    Ok(())
}

fn run_providers(json: bool) {
    let rows: Vec<serde_json::Value> = providers::all()
        .iter()
        .map(|p| {
            let status = match p.binary() {
                Ok(path) => serde_json::json!({"installed": true, "path": path.to_string_lossy()}),
                Err(e) => serde_json::json!({"installed": false, "hint": e.to_string()}),
            };
            serde_json::json!({
                "id": p.id(),
                "display": p.display(),
                "platforms": provider_platforms(p.id()),
                "status": status,
            })
        })
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&rows).unwrap());
        return;
    }
    println!("{}", output::bold("provider backends:"));
    for row in &rows {
        let id = row.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let display = row.get("display").and_then(|v| v.as_str()).unwrap_or("");
        let plats = row
            .get("platforms")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let status = &row["status"];
        if status
            .get("installed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let path = status.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            println!(
                "  {} {} {} {}",
                output::ok(id),
                output::dim(display),
                output::dim(&format!("[{plats}]")),
                output::dim(path)
            );
        } else {
            let hint = status.get("hint").and_then(|v| v.as_str()).unwrap_or("");
            println!(
                "  {} {} {} {}",
                output::dim(&format!("○ {id}")),
                output::dim(display),
                output::dim(&format!("[{plats}]")),
                output::dim(hint)
            );
        }
    }
    println!(
        "\n{}",
        output::dim("set per model: `modeldesk use <name> --provider <id>`")
    );
}

fn run_pull(cfg: &Config, name: Option<&str>, json: bool) -> Result<()> {
    let entry = match name {
        Some(n) => cfg
            .resolve(n)
            .ok_or_else(|| ModeldeskError::UnknownModel { name: n.to_string() })?,
        None => cfg.active_entry().map_err(|_| ModeldeskError::UnknownModel {
            name: cfg.active.clone().unwrap_or_default(),
        })?,
    };
    compat::check(&entry, cfg.port)?;
    let provider = providers::for_entry(&entry);
    let bin = provider.binary().map_err(|_| ModeldeskError::ProviderMissing {
        provider: entry.provider.clone(),
        hint: String::new(),
    })?;
    let need_gb = compat::estimate_need_gb(&entry);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "profile": entry.name, "repo": entry.repo,
                "estimate_gb": need_gb,
            }))?
        );
    } else {
        println!(
            "{} {} (~{need_gb:.1} GB) via {}",
            output::cyan("●"),
            entry.repo,
            entry.provider
        );
    }
    if entry.provider != "ollama" {
        println!(
            "{}",
            output::dim("this backend fetches weights on first start — nothing to pre-pull; `modeldesk start` will download with progress")
        );
        return Ok(());
    }
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::with_template("{spinner:.green} {msg}")?);
    pb.set_message(format!("ollama pull {}", entry.repo));
    pb.enable_steady_tick(Duration::from_millis(100));
    let status = std::process::Command::new(&bin)
        .args(["pull", &entry.repo])
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;
    pb.finish_and_clear();
    if !status.success() {
        anyhow::bail!("`ollama pull {}` failed ({status})", entry.repo);
    }
    println!("{} {}", output::ok("cached"), entry.repo);
    Ok(())
}

fn run_doctor(cfg: &Config, name: Option<&str>, json: bool) -> Result<()> {
    let entry = match name {
        Some(n) => cfg
            .resolve(n)
            .ok_or_else(|| ModeldeskError::UnknownModel { name: n.to_string() })?,
        None => cfg.active_entry().map_err(|_| ModeldeskError::UnknownModel {
            name: cfg.active.clone().unwrap_or_default(),
        })?,
    };
    let mut checks: Vec<serde_json::Value> = Vec::new();
    let mut failed = false;

    let provider = providers::for_entry(&entry);
    match provider.binary() {
        Ok(path) => checks.push(serde_json::json!({
            "check": "provider-installed", "ok": true,
            "detail": path.to_string_lossy(),
        })),
        Err(_) => {
            failed = true;
            checks.push(serde_json::json!({
                "check": "provider-installed", "ok": false,
                "detail": format!("backend '{}' not found", entry.provider),
            }));
        }
    }

    let format = compat::detect_format(&entry.repo);
    match compat::provider_accepts(&entry.provider, format, &entry.repo) {
        Ok(()) => checks.push(serde_json::json!({
            "check": "format-compatible", "ok": true,
            "detail": format!("{format:?} weights → {}", entry.provider),
        })),
        Err(reason) => {
            failed = true;
            checks.push(serde_json::json!({
                "check": "format-compatible", "ok": false, "detail": reason,
            }));
        }
    }

    match compat::arch_ok(&entry.provider) {
        Ok(()) => checks.push(serde_json::json!({
            "check": "arch-supported", "ok": true,
            "detail": format!("{} on {}", entry.provider, platform::os_label()),
        })),
        Err(reason) => {
            failed = true;
            checks.push(serde_json::json!({
                "check": "arch-supported", "ok": false, "detail": reason,
            }));
        }
    }

    let need_gb = compat::estimate_need_gb(&entry);
    let have_gb = compat::total_memory_gb();
    if need_gb > have_gb * 0.85 {
        failed = true;
        checks.push(serde_json::json!({
            "check": "memory", "ok": false,
            "detail": format!("needs ~{need_gb:.1} GB, machine has {have_gb:.0} GB"),
        }));
    } else {
        let mut detail = format!("needs ~{need_gb:.1} GB of {have_gb:.0} GB");
        if need_gb > have_gb * 0.6 {
            detail += " (tight — close other apps)";
        }
        checks.push(serde_json::json!({
            "check": "memory", "ok": true, "detail": detail,
        }));
    }

    match server::server_status(cfg) {
        ServerStatus::Stopped => checks.push(serde_json::json!({
            "check": "port", "ok": true,
            "detail": format!(":{} free", cfg.port),
        })),
        ServerStatus::Running { pid, .. } => {
            let detail = if pid == 0 {
                let holder = compat::port_holder(cfg.port).unwrap_or_else(|| "another process".to_string());
                format!(":{} held by {holder} (modeldesk did not start it — stop it first or hot-swap will fail)", cfg.port)
            } else {
                format!(":{port} held by pid {pid} (hot-swap or restart will reuse it)", port = cfg.port)
            };
            checks.push(serde_json::json!({
                "check": "port", "ok": true, "detail": detail,
            }));
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "profile": entry.name, "provider": entry.provider,
                "repo": entry.repo, "ok": !failed, "checks": checks,
            }))?
        );
    } else {
        println!(
            "{} {} [{}]",
            if failed { output::err("fail") } else { output::ok("pass") },
            entry.name,
            entry.provider
        );
        for c in &checks {
            let ok = c.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            let name = c.get("check").and_then(|v| v.as_str()).unwrap_or("?");
            let detail = c.get("detail").and_then(|v| v.as_str()).unwrap_or("");
            let mark = if ok { output::ok(name) } else { output::err(name) };
            println!("  {mark} {}", output::dim(detail));
        }
    }
    if failed {
        Err(ModeldeskError::Incompatible {
            repo: entry.repo.clone(),
            provider: entry.provider.clone(),
            reason: "doctor checks failed (see above)".to_string(),
        }
        .into())
    } else {
        Ok(())
    }
}

fn run_profile(cfg: &mut Config, command: ProfileCommands, json: bool) -> Result<()> {
    match command {
        ProfileCommands::List => {
            if json {
                println!("{}", serde_json::to_string_pretty(&cfg.models)?);
                return Ok(());
            }
            println!("{}", output::bold("profiles:"));
            for m in &cfg.models {
                let mark = if Some(&m.name) == cfg.active.as_ref() {
                    output::cyan("*")
                } else {
                    " ".to_string()
                };
                let extra = [
                    m.drafter.as_ref().map(|d| format!("drafter={d}")),
                    Some(format!("kv={}", m.kv_bits)),
                    m.context_window.map(|c| format!("ctx={c}")),
                    m.default_temperature.map(|t| format!("temp={t}")),
                    m.mode.clone().map(|v| format!("mode={v}")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
                println!(
                    "  {mark} {} [{}] {} {}",
                    m.name,
                    m.provider,
                    output::dim(&m.repo),
                    output::dim(&extra)
                );
            }
        }
        ProfileCommands::Show { name } => {
            let m = cfg.resolve(&name).ok_or_else(|| ModeldeskError::UnknownModel {
                name: name.clone(),
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&m)?);
                return Ok(());
            }
            println!("{} {}", output::bold(&m.name), output::dim(&format!("[{}]", m.provider)));
            println!("  repo:        {}", m.repo);
            println!(
                "  drafter:     {}",
                m.drafter.as_deref().unwrap_or("(none)")
            );
            println!("  kv_bits:     {}", m.kv_bits);
            println!(
                "  ctx window:  {}",
                m.context_window.map(|c| c.to_string()).as_deref().unwrap_or("(model default)")
            );
            println!(
                "  temperature: {}",
                m.default_temperature.map(|t| t.to_string()).as_deref().unwrap_or("(model default)")
            );
            println!(
                "  mode:        {}",
                m.mode.as_deref().unwrap_or("(provider default)")
            );
        }
        ProfileCommands::Add {
            name,
            repo,
            provider,
            drafter,
            kv_bits,
            context_window,
            temperature,
            mode,
            overwrite,
        } => {
            if cfg.resolve(&name).is_some() && !overwrite {
                return Err(ModeldeskError::ProfileExists { name }.into());
            }
            let backend = provider.unwrap_or_else(|| {
                if repo.to_lowercase().ends_with(".gguf") || repo.contains(".gguf") {
                    "llama-cpp".to_string()
                } else {
                    "mlx-dspark".to_string()
                }
            });
            if !providers::all().iter().any(|p| p.id() == backend) {
                return Err(ModeldeskError::UnknownProvider { id: backend }.into());
            }
            if !matches!(kv_bits, 0 | 4 | 8) {
                anyhow::bail!("kv-bits must be 0, 4, or 8");
            }
            let candidate = ModelEntry {
                name: name.clone(),
                provider: backend.clone(),
                repo: repo.clone(),
                drafter: drafter.clone(),
                kv_bits,
                context_window,
                default_temperature: temperature,
                mode: mode.clone(),
            };
            compat::check(&candidate, cfg.port).map_err(|e| {
                anyhow::anyhow!("profile '{name}' rejected: {e:#}")
            })?;
            if overwrite {
                cfg.models.retain(|m| m.name != name);
            }
            cfg.models.push(candidate);
            if cfg.active.is_none() {
                cfg.active = Some(name.clone());
            }
            cfg.save()?;
            println!(
                "{} profile '{name}' saved [{}] ({})",
                output::ok("saved"),
                backend,
                output::dim(&repo)
            );
        }
        ProfileCommands::Remove { name } => {
            if cfg.resolve(&name).is_none() {
                return Err(ModeldeskError::UnknownModel { name }.into());
            }
            cfg.models.retain(|m| m.name != name);
            if cfg.active.as_deref() == Some(&name) {
                cfg.active = cfg.models.first().map(|m| m.name.clone());
            }
            cfg.save()?;
            println!("{} profile '{name}' removed", output::ok("done"));
        }
    }
    Ok(())
}

fn hint_for(e: &anyhow::Error) -> Option<String> {
    if let Some(md) = e.downcast_ref::<ModeldeskError>() {
        return md.hint();
    }
    for cause in e.chain().skip(1) {
        if let Some(md) = cause.downcast_ref::<ModeldeskError>() {
            return md.hint();
        }
    }
    None
}

fn provider_platforms(id: &str) -> Vec<&'static str> {
    match id {
        "mlx-dspark" | "mlx-lm" => vec!["macos"],
        "llama-cpp" | "ollama" => vec!["macos", "linux", "windows"],
        _ => vec!["macos"],
    }
}
