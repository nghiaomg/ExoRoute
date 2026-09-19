use super::*;

pub(crate) async fn async_main() -> Result<(), Box<dyn std::error::Error>> {
    let startup_started = std::time::Instant::now();
    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "serve".to_owned());
    match command.as_str() {
        "version" | "--version" | "-V" => {
            println!("ExoRoute {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        "check" | "serve" => {}
        other => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unknown command '{other}'. Use 'serve', 'check', or 'version'."),
            )
            .into());
        }
    }

    let app_dir = crate::config::app_dir()?;
    crate::config::initialize_app_dir(&app_dir)?;
    let admin_key_was_set_in_process = std::env::var_os("EXOROUTE_ADMIN_KEY").is_some();
    let master_key_was_set_in_process = std::env::var_os("EXOROUTE_MASTER_KEY").is_some();
    dotenvy::from_path(app_dir.join(".env"))?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let mut config = Config::from_env_deferred_operational(app_dir)?;
    if master_key_was_set_in_process && config.master_key.is_none() {
        return Err("EXOROUTE_MASTER_KEY is explicitly empty; remove the empty environment override to allow automatic key generation, or configure a stable 32-byte key.".into());
    }
    ensure_supported_bind(&config)?;
    if command == "check" {
        return check_configuration(config).await;
    }
    crate::config::ensure_private_dir(&config.data_dir)?;
    let _database_lock = crate::infra::db::lock_database(&config.database_path)?;
    let (master_key, master_key_generated) =
        crate::config::ensure_master_key(&config.app_dir.join(".env"), config.master_key)?;
    config.master_key = Some(master_key);
    if master_key_generated {
        println!(
            "A random EXOROUTE_MASTER_KEY was generated and stored in {}. Keep this file private and preserve the key for backups.",
            config.app_dir.join(".env").display()
        );
    }
    if let Err(error) = crate::admin::cleanup_stale_database_temp_dirs(&config.app_dir) {
        tracing::warn!(%error, "could not remove temporary LMDB backup directories from a previous run");
    }
    let db = crate::infra::db::connect(&config.database_path).await?;
    let gateway_resource_limits = crate::infra::db::load_gateway_resource_limits(&db)
        .await
        .map_err(std::io::Error::other)?;
    let stored_operational = crate::infra::db::load_operational_settings(&db)
        .await
        .map_err(std::io::Error::other)?;
    let output_styles = crate::infra::db::load_output_styles(&db)
        .await
        .map_err(std::io::Error::other)?;
    let env_operational = OperationalSettings::from_env();
    let effective_operational = if stored_operational.overridden {
        stored_operational.settings
    } else {
        env_operational
            .as_ref()
            .copied()
            .map_err(|error| std::io::Error::other(error.clone()))?
    };
    let operational_record = OperationalSettingsRecord {
        settings: effective_operational,
        revision: stored_operational.revision,
        overridden: stored_operational.overridden,
    };
    apply_operational_settings_to_config(&mut config, effective_operational);
    let has_database_auth = crate::infra::db::has_admin_auth(&db).await?;
    if config.admin_key.is_none() && !has_database_auth {
        if admin_key_was_set_in_process {
            return Err("EXOROUTE_ADMIN_KEY is explicitly empty; configure a non-empty value or remove the environment override.".into());
        }
        let (key, generated) = crate::config::ensure_admin_key(&config.app_dir.join(".env"), None)?;
        config.admin_key = Some(key.clone());
        if generated {
            if std::io::stdout().is_terminal() {
                eprintln!(
                    "\nGenerated initial ExoRoute admin password: {key}\nIt is also stored in {}. Keep it private.\n",
                    config.app_dir.join(".env").display()
                );
            } else {
                println!(
                    "A random ExoRoute admin password was generated and stored in {}.",
                    config.app_dir.join(".env").display()
                );
            }
        }
    }
    if !has_database_auth
        && config
            .admin_key
            .as_deref()
            .map(crate::admin::password_needs_change)
            .unwrap_or(true)
    {
        tracing::warn!(
            "Legacy or weak admin password is active; gateway traffic stays disabled until it is changed through the admin login."
        );
    }
    let addr = config.bind;
    let state = AppState::new_with_runtime_settings(
        config,
        db,
        gateway_resource_limits,
        operational_record,
        env_operational,
        output_styles,
    );
    tokio::spawn(request_log_retention_worker(state.clone()));
    tokio::spawn(crate::admin::providers::provider_deletion_recovery_worker(
        state.clone(),
    ));
    let database_requires_password_change =
        crate::infra::db::admin_password_change_required(&state.db).await?;
    if let Some(required) = database_requires_password_change {
        state
            .admin
            .admin_password_change_required
            .store(required, std::sync::atomic::Ordering::Release);
    }
    let must_change_password = state
        .admin
        .admin_password_change_required
        .load(std::sync::atomic::Ordering::Acquire);
    let config_ref = state.config.clone();
    let app = router(state.clone());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    #[cfg(windows)]
    crate::config::clear_windows_acl_cache();
    crate::app::banner::print_startup(
        addr,
        startup_started.elapsed(),
        &config_ref,
        must_change_password,
    );
    if addr.ip().is_unspecified() {
        tracing::warn!(
            "Listening on all IPv4 interfaces over plain HTTP; restrict this port to trusted test clients with host and network firewalls."
        );
    }
    let result =
        serve_with_shutdown_signal(listener, app, state.clone(), shutdown_signal(state.clone()))
            .await;
    let telemetry_result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        state.telemetry.shutdown(),
    )
    .await
    .unwrap_or_else(|_| {
        Err("Telemetry shutdown timed out before the writer could drain.".to_owned())
    });
    if let Err(error) = telemetry_result {
        tracing::warn!(%error, "telemetry did not drain before shutdown");
    }
    result?;
    Ok(())
}

pub(crate) async fn check_configuration(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    crate::config::ensure_private_dir(&config.data_dir)?;
    // `check` initializes the schema and prunes logs, so it must obey the same
    // single-process database rule as `serve`.
    let _database_lock = crate::infra::db::lock_database(&config.database_path)?;
    let probe = config
        .data_dir
        .join(format!(".write-check-{}", uuid::Uuid::new_v4()));
    tokio::fs::write(&probe, b"ok").await?;
    tokio::fs::remove_file(&probe).await?;
    println!("✓ Environment configuration");
    println!(
        "✓ ExoRoute settings: {}",
        config.app_dir.join(".env").display()
    );
    println!("✓ Writable data directory: {}", config.data_dir.display());
    let db = crate::infra::db::connect(&config.database_path).await?;
    let stored_operational = crate::infra::db::load_operational_settings(&db)
        .await
        .map_err(std::io::Error::other)?;
    let effective_operational = if stored_operational.overridden {
        stored_operational.settings
    } else {
        OperationalSettings::from_env()?
    };
    effective_operational
        .validate()
        .map_err(std::io::Error::other)?;
    println!(
        "✓ LMDB environment initialized: {}",
        config.database_path.display()
    );
    println!(
        "✓ Operational settings validated ({})",
        if stored_operational.overridden {
            "database override"
        } else {
            "environment/default"
        }
    );
    let database_requires_password_change =
        crate::infra::db::admin_password_change_required(&db).await?;
    if config.master_key.is_some() {
        println!("✓ Provider credential encryption key configured");
    } else {
        println!("! EXOROUTE_MASTER_KEY is not set; provider credentials cannot be saved");
    }
    let password_change_required = database_requires_password_change.unwrap_or_else(|| {
        config
            .admin_key
            .as_deref()
            .map(crate::admin::password_needs_change)
            .unwrap_or(true)
    });
    if !password_change_required {
        println!("✓ Admin authentication configured with a strong password");
    } else {
        println!("! Admin password must be changed before gateway traffic is enabled");
    }
    println!("✓ Listener bind address: {}", config.bind);
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    drop(listener);
    println!("✓ Bind address is available: {}", config.bind);
    println!("Ready.");
    Ok(())
}

pub(crate) fn apply_operational_settings_to_config(
    config: &mut Config,
    settings: OperationalSettings,
) {
    config.connect_timeout = settings.connect_timeout;
    config.request_timeout = settings.request_timeout;
    config.stream_idle_timeout = settings.stream_idle_timeout;
    config.circuit_breaker_enabled = settings.circuit_breaker_enabled;
    config.circuit_breaker_threshold = settings.circuit_breaker_threshold;
    config.circuit_breaker_cooldown = settings.circuit_breaker_cooldown;
}

pub(crate) fn ensure_supported_bind(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let ip = config.bind.ip();
    if ip.is_loopback() {
        return Ok(());
    }
    Err(format!(
        "Unsupported bind address {}. ExoRoute serves plain HTTP and only supports loopback addresses; use a TLS reverse proxy for remote access.",
        config.bind
    )
    .into())
}
