use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::BufRead;
use std::time::Duration;

use modelsdesk::cli::{Cli, Commands, ProfileCommands};
use modelsdesk::compat;
use modelsdesk::config::{Config, ModelEntry};
use modelsdesk::connect;
use modelsdesk::error::ModeldeskError;
use modelsdesk::providers::ServerStatus;
use modelsdesk::{agents, freshness, hub, models, output, platform, providers, server, watch};

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
                "modelsdesk targets macOS first; running on {} (provider-agnostic core, YMMV)",
                platform::os_label()
            ))
        );
    }

    // Ambient outdated warnings: every command except the checker itself,
    // completions, and machine-readable runs. Cache-gated (6h), non-blocking.
    // (Computed before the match moves `cli.command`.)
    let ambient_skip = matches!(cli.command, Commands::Outdated { .. })
        || matches!(cli.command, Commands::Completions { .. })
        || cli.json;

    match cli.command {
        Commands::Start {
            foreground,
            no_install,
        } => {
            if no_install {
                std::env::set_var("MODELDESK_NO_INSTALL", "1");
            }
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
                    let tty = console::Term::stdout().is_term();
                    let pb = ProgressBar::new_spinner();
                    if tty {
                        pb.set_style(ProgressStyle::with_template("{spinner:.red} {msg}")?);
                        pb.set_message(format!("stopping pid {pid} on :{port}"));
                        pb.enable_steady_tick(Duration::from_millis(80));
                    } else {
                        pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
                        println!("stopping pid {pid} on :{port}…");
                    }
                    server::stop(&cfg).await?;
                    let done = output::ok(&format!("port :{port} free"));
                    if tty {
                        pb.finish_with_message(done);
                    } else {
                        println!("{done}");
                    }
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
        Commands::Search { query, limit } => {
            run_search(&query.join(" "), limit, cli.json).await?;
        }
        Commands::Setup {
            repo,
            name,
            provider,
            drafter,
            start,
            no_install,
        } => {
            if no_install {
                std::env::set_var("MODELDESK_NO_INSTALL", "1");
            }
            let mut cfg = Config::load()?;
            run_setup(
                &mut cfg,
                &repo,
                name.as_deref(),
                provider.as_deref(),
                drafter.as_deref(),
                start,
                cli.json,
            )
            .await?;
        }
        Commands::Doctor { name, fix } => {
            let cfg = Config::load()?;
            run_doctor(&cfg, name.as_deref(), fix, cli.json)?;
        }
        Commands::SyncAgents => {
            let cfg = Config::load()?;
            agents::sync(&cfg, false, cli.json)?;
        }
        Commands::Outdated { refresh_cache } => {
            let cfg = Config::load()?;
            if refresh_cache {
                // Background refresh worker: silent, cache-only, never warns.
                let report = freshness::full_check(&cfg).await;
                let _ = report;
                return Ok(());
            }
            run_outdated(&cfg, cli.json).await?;
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
        Commands::Connect { agent, list, dry_run } => {
            run_connect(agent.as_deref(), list, dry_run)?;
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let shell = shell.to_lowercase();
            match shell.as_str() {
                "bash" => generate(
                    clap_complete::shells::Bash,
                    &mut cmd,
                    "modelsdesk",
                    &mut std::io::stdout(),
                ),
                "zsh" => generate(
                    clap_complete::shells::Zsh,
                    &mut cmd,
                    "modelsdesk",
                    &mut std::io::stdout(),
                ),
                "fish" => generate(
                    clap_complete::shells::Fish,
                    &mut cmd,
                    "modelsdesk",
                    &mut std::io::stdout(),
                ),
                "powershell" => generate(
                    clap_complete::shells::PowerShell,
                    &mut cmd,
                    "modelsdesk",
                    &mut std::io::stdout(),
                ),
                "elvish" => generate(
                    clap_complete::shells::Elvish,
                    &mut cmd,
                    "modelsdesk",
                    &mut std::io::stdout(),
                ),
                _ => anyhow::bail!("unknown shell '{shell}' (bash, zsh, fish, powershell, elvish)"),
            }
        }
    }
    if !ambient_skip {
        if let Ok(cfg) = Config::load() {
            freshness::maybe_warn(&cfg);
        }
    }
    Ok(())
}

fn run_connect(agent: Option<&str>, list: bool, dry_run: bool) -> Result<()> {
    if list || agent.is_none() {
        println!("{}", output::bold("supported agents:"));
        for a in connect::AGENTS {
            let path = connect::config_path_for(a)?;
            let state = if path.exists() { "" } else { " (not installed?)" };
            println!("  {a:<10} {}{}", output::dim(&path.display().to_string()), output::dim(state));
        }
        for (name, why) in connect::UNSUPPORTED {
            println!("  {name:<10} {}", output::dim(why));
        }
        if agent.is_none() && !list {
            anyhow::bail!("pass an agent: `modelsdesk connect <jcode|codex|opencode>`");
        }
        return Ok(());
    }
    let agent = agent.unwrap();
    if !connect::supported(agent) {
        anyhow::bail!(
            "unknown agent '{agent}' (supported: {})",
            connect::AGENTS.join(", ")
        );
    }
    let cfg = Config::load()?;
    let ep = connect::Endpoint::from_config(&cfg)?;
    let path = connect::config_path_for(agent)?;
    println!(
        "{} {agent} → {} ({})",
        output::cyan("●"),
        ep.base_url,
        path.display()
    );
    if dry_run {
        println!("{}", output::dim("dry-run: no files written"));
        return Ok(());
    }
    connect::write_for(agent, &path, &ep)?;
    println!("{}", output::ok(&format!("{agent} configured — restart {agent} to pick it up")));
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
        println!("  {}", output::dim("(none found in HF cache)"));
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
            anyhow::bail!("unknown provider '{want}' (see `modelsdesk providers`)",);
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
        cfg.models.push(modelsdesk::config::ModelEntry {
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
            output::dim("server stopped — `modelsdesk start` to load it")
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
    let tty = console::Term::stdout().is_term();
    let pb = ProgressBar::new_spinner();
    if tty {
        pb.set_style(ProgressStyle::with_template("{spinner:.green} {msg}")?);
        pb.set_message(format!("hot-swapping to {}", entry.repo));
        pb.enable_steady_tick(Duration::from_millis(80));
    } else {
        pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        println!("hot-swapping to {}…", entry.repo);
    }
    let done = |msg: String| {
        if tty {
            pb.finish_with_message(msg.clone());
        } else {
            pb.finish_and_clear();
            println!("{msg}");
        }
    };
    match provider
        .admin_load(&client, &cfg.host, cfg.port, &entry)
        .await
    {
        Ok(true) => done(output::ok(&format!("now serving {}", entry.repo))),
        _ => {
            done(output::warn("hot-swap unsupported — restarting"));
            server::stop(cfg).await?;
            watch::animated_boot(cfg).await?;
        }
    }
    if !json {
        if let Err(e) = agents::sync(cfg, true, false) {
            println!("{} agent sync skipped: {e:#}", output::dim("→"));
        }
    }
    Ok(())
}

fn run_providers(json: bool) {
    use modelsdesk::runtime_env::Env;
    let env = Env::detect();
    let rows: Vec<serde_json::Value> = providers::all()
        .iter()
        .map(|p| {
            let (status, version) = match p.binary() {
                Ok(path) => {
                    let ver = p.installed_version().unwrap_or(None);
                    (
                        serde_json::json!({"installed": true, "path": path.to_string_lossy()}),
                        ver,
                    )
                }
                Err(e) => (
                    serde_json::json!({"installed": false, "hint": e.to_string()}),
                    None,
                ),
            };
            let mut row = serde_json::json!({
                "id": p.id(),
                "display": p.display(),
                "platforms": provider_platforms(p.id()),
                "supported_here": env.supports(p.id()),
                "status": status,
            });
            if let Some(v) = version {
                row["version"] = serde_json::json!(v);
                if let Some(min) = p.min_version() {
                    row["min_version"] =
                        serde_json::json!(format!("{}.{}.{}", min.0, min.1, min.2));
                    row["version_ok"] =
                        serde_json::json!(providers::version_meets(&v, min));
                }
            }
            row
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
            let ver = row
                .get("version")
                .and_then(|v| v.as_str())
                .map(|v| format!("v{v}"))
                .unwrap_or_default();
            let vok = match row.get("version_ok").and_then(|v| v.as_bool()) {
                Some(true) => String::new(),
                Some(false) => {
                    let want = row.get("min_version").and_then(|v| v.as_str()).unwrap_or("?");
                    format!(" {}", output::warn(&format!("old, want ≥{want}")))
                }
                None => String::new(),
            };
            let sup = if row.get("supported_here").and_then(|v| v.as_bool()).unwrap_or(true) {
                String::new()
            } else {
                format!(" {}", output::err("unsupported here"))
            };
            println!(
                "  {} {} {} {} {ver}{vok}{sup}",
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
        output::dim("set per model: `modelsdesk use <name> --provider <id>`")
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
            output::dim("this backend fetches weights on first start — nothing to pre-pull; `modelsdesk start` will download with progress")
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

fn run_doctor(cfg: &Config, name: Option<&str>, fix: bool, json: bool) -> Result<()> {
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
        Ok(path) => {
            let mut detail = path.to_string_lossy().to_string();
            match provider.installed_version().unwrap_or(None) {
                Some(v) => {
                    detail.push_str(&format!(" (v{v})"));
                    if let Some(min) = provider.min_version() {
                        if providers::version_meets(&v, min) {
                            detail.push_str(" ✓");
                        } else {
                            detail.push_str(&format!(
                                " (old, want ≥{}.{}.{})",
                                min.0, min.1, min.2
                            ));
                        }
                    }
                }
                None => detail.push_str(" (version unknown)"),
            }
            checks.push(serde_json::json!({
                "check": "provider-installed", "ok": true, "detail": detail,
            }));
        }
        Err(_) => {
            if fix && !modelsdesk::runtime_install::no_install_mode() {
                use modelsdesk::runtime_env::Env;
                let env = Env::detect();
                match modelsdesk::runtime_install::plan(provider.id(), &env)
                    .and_then(|p| modelsdesk::runtime_install::run_plan(&p).map(|_| p))
                {
                    Ok(_) => {
                        checks.push(serde_json::json!({
                            "check": "provider-installed", "ok": true,
                            "detail": "installed via --fix",
                        }));
                    }
                    Err(e) => {
                        failed = true;
                        checks.push(serde_json::json!({
                            "check": "provider-installed", "ok": false,
                            "detail": format!("backend '{}' not found; install failed: {e:#}", entry.provider),
                        }));
                    }
                }
            } else {
                failed = true;
                checks.push(serde_json::json!({
                    "check": "provider-installed", "ok": false,
                    "detail": format!("backend '{}' not found (rerun with --fix to install)", entry.provider),
                }));
            }
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
                format!(":{} held by {holder} (modelsdesk did not start it — stop it first or hot-swap will fail)", cfg.port)
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

async fn run_search(query: &str, limit: usize, json: bool) -> Result<()> {
    if query.trim().is_empty() {
        anyhow::bail!("give a search query: `modelsdesk search qwen3 8b`");
    }
    let tty = console::Term::stdout().is_term();
    let pb = ProgressBar::new_spinner();
    if tty {
        pb.set_style(ProgressStyle::with_template("{spinner:.green} {msg}")?);
        pb.set_message(format!("searching huggingface for '{query}'"));
        pb.enable_steady_tick(Duration::from_millis(80));
    }
    let hits = hub::search(query, limit).await?;
    if tty {
        pb.finish_and_clear();
    }
    let enriched = hub::enrich(hits);
    hub::print_hits(&enriched, json)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_setup(
    cfg: &mut Config,
    repo: &str,
    name: Option<&str>,
    provider_override: Option<&str>,
    drafter: Option<&str>,
    start: bool,
    json: bool,
) -> Result<()> {
    let is_local_path = repo.starts_with('.') || repo.starts_with('/') || repo.starts_with('~');
    let format = compat::detect_format(repo);
    let backend = provider_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| hub::recommend_provider(format).to_string());
    if !providers::all().iter().any(|p| p.id() == backend) {
        return Err(ModeldeskError::UnknownProvider { id: backend }.into());
    }
    let short = name
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            repo.trim_end_matches('/')
                .split('/')
                .next_back()
                .unwrap_or(repo)
                .to_string()
        });

    if !is_local_path && !repo.contains('/') {
        // Bare ollama-style name: no HF repo to verify, backend must be ollama.
        if backend != "ollama" {
            anyhow::bail!(
                "'{repo}' looks like an ollama model name — pass --provider ollama or use a full HF repo id"
            );
        }
    } else if !is_local_path {
        println!("{} verifying {repo} on huggingface…", output::dim("→"));
        let files = hub::repo_files(repo).await?;
        let has_weights = files.iter().any(|f| {
            f.ends_with(".safetensors")
                || f.ends_with(".safetensors.index.json")
                || f.ends_with(".gguf")
        });
        if !has_weights {
            anyhow::bail!("repo '{repo}' has no weight files (.safetensors/.gguf) — wrong repo?");
        }
        println!(
            "{}",
            output::dim(&format!("repo ok ({} files)", files.len()))
        );
    } else {
        let expanded = shellexpand(repo);
        if !std::path::Path::new(&expanded).exists() {
            anyhow::bail!("local path '{repo}' does not exist");
        }
    }

    if backend == "mlx-dspark" && drafter.is_none() && format == compat::ModelFormat::MlX {
        println!(
            "{}",
            output::dim("tip: add --drafter <repo> for speculative-decoding speedup (optional)")
        );
    }

    let candidate = ModelEntry {
        name: short.clone(),
        provider: backend.clone(),
        repo: repo.to_string(),
        drafter: drafter.map(|s| s.to_string()),
        kv_bits: 4,
        context_window: None,
        default_temperature: None,
        mode: None,
    };
    compat::check(&candidate, cfg.port).map_err(|e| {
        anyhow::anyhow!("'{repo}' rejected: {e:#}")
    })?;

    if cfg.resolve(&short).is_some() {
        cfg.models.retain(|m| m.name != short);
        println!("{}", output::dim(&format!("replacing existing profile '{short}'")));
    }
    cfg.models.push(candidate);
    cfg.active = Some(short.clone());
    cfg.save()?;
    println!(
        "{} profile '{short}' saved [{backend}] ({repo})",
        output::ok("setup")
    );

    if backend == "ollama" && !hub::is_cached(repo) {
        println!("{} pulling {repo}…", output::dim("→"));
        run_pull(cfg, Some(&short), false)?;
    } else if !hub::is_cached(repo) && !is_local_path {
        println!(
            "{}",
            output::dim("weights download on first `modelsdesk start` (or `modelsdesk pull`)")
        );
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "active": short, "provider": backend, "repo": repo,
            }))?
        );
    }
    if start {
        println!("{} starting {short}…", output::dim("→"));
        watch::animated_boot(cfg).await?;
    } else {
        println!(
            "{}",
            output::dim(&format!("`modelsdesk use {short}` to switch, `modelsdesk start` to serve"))
        );
    }
    Ok(())
}

fn shellexpand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    p.to_string()
}

async fn run_outdated(cfg: &Config, json: bool) -> Result<()> {
    let tty = console::Term::stdout().is_term();
    let pb = ProgressBar::new_spinner();
    if tty {
        pb.set_style(ProgressStyle::with_template("{spinner:.green} {msg}")?);
        pb.set_message("checking backends + cached models…");
        pb.enable_steady_tick(Duration::from_millis(80));
    }
    let mut backends = Vec::new();
    for p in providers::all() {
        backends.push(freshness::check_backend(&*p).await);
    }
    let mut models_out = Vec::new();
    for m in models::local_models(cfg) {
        models_out.push(freshness::check_model(&m.repo).await);
    }
    if tty {
        pb.finish_and_clear();
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "backends": backends, "models": models_out,
            }))?
        );
        return Ok(());
    }
    println!("{}", output::bold("backends:"));
    let mut all_current = true;
    for b in &backends {
        use freshness::Freshness as F;
        let line = match b.status {
            F::Current => format!(
                "  {} {} v{}",
                output::ok(&b.backend),
                output::dim("current"),
                b.installed.as_deref().unwrap_or("?")
            ),
            F::UpdateAvailable => {
                all_current = false;
                format!(
                    "  {} v{} → v{}  {}",
                    output::warn(&b.backend),
                    b.installed.as_deref().unwrap_or("?"),
                    b.latest.as_deref().unwrap_or("?"),
                    output::dim(b.update_cmd.as_deref().unwrap_or(""))
                )
            }
            F::BelowMinimum => {
                all_current = false;
                format!(
                    "  {} v{} {}  {}",
                    output::err(&b.backend),
                    b.installed.as_deref().unwrap_or("?"),
                    output::err("below minimum — upgrade now"),
                    output::dim(b.update_cmd.as_deref().unwrap_or(""))
                )
            }
            F::Unknown => format!(
                "  {} {} {}",
                output::dim(&format!("? {}", b.backend)),
                output::dim(&format!(
                    "v{} (latest unknown — offline?)",
                    b.installed.as_deref().unwrap_or("?")
                )),
                output::dim("")
            ),
            F::NotInstalled => format!(
                "  {} {}",
                output::dim(&format!("○ {}", b.backend)),
                output::dim("not installed")
            ),
        };
        println!("{line}");
    }
    println!("\n{}", output::bold("cached models:"));
    for m in &models_out {
        use freshness::Freshness as F;
        match m.status {
            F::Current => println!(
                "  {} {} {}",
                output::ok(&m.repo),
                output::dim("up to date"),
                output::dim(m.cached_snapshot.as_deref().unwrap_or(""))
            ),
            F::UpdateAvailable => {
                all_current = false;
                println!(
                    "  {} {} (upstream {})  {}",
                    output::warn(&m.repo),
                    output::dim("stale"),
                    output::dim(m.upstream_modified.as_deref().unwrap_or("?")),
                    output::dim("re-pull to refresh")
                );
            }
            F::Unknown | F::BelowMinimum => println!(
                "  {} {}",
                output::dim(&m.repo),
                output::dim("upstream unreachable — cannot compare")
            ),
            F::NotInstalled => println!("  {} {}", output::dim(&m.repo), output::dim("not cached")),
        }
    }
    if all_current {
        println!("\n{}", output::ok("everything current"));
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
        "llama-cpp" | "ollama" | "strata" | "mistral-rs" => {
            vec!["macos", "linux", "windows"]
        }
        "vllm" => vec!["linux", "windows"],
        "sglang" => vec!["linux"],
        _ => vec!["macos"],
    }
}
