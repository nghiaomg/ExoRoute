use super::*;

#[cfg(windows)]
#[test]
fn windows_private_acl_can_be_applied_repeatedly() {
    let app_dir = env::temp_dir().join(format!("exoroute-acl-{}", uuid::Uuid::new_v4()));
    ensure_private_dir(&app_dir).expect("secure temporary data directory");
    ensure_private_dir(&app_dir).expect("reapply directory ACL");

    let secret_path = app_dir.join("secret.env");
    fs::write(&secret_path, b"test-only").expect("create temporary secret file");
    ensure_private_file(&secret_path).expect("secure temporary secret file");
    ensure_private_file(&secret_path).expect("reapply file ACL");

    let moved_secret_path = app_dir.join("secret.previous");
    fs::rename(&secret_path, &moved_secret_path).expect("move prior temporary secret");
    fs::write(&secret_path, b"replacement").expect("create replacement secret file");
    ensure_private_file(&secret_path).expect("secure replacement file with a new identity");

    fs::remove_dir_all(app_dir).expect("remove temporary ACL test directory");
}

#[test]
fn operational_defaults_and_limits_are_validated() {
    let defaults = OperationalSettings::default();
    assert_eq!(defaults.connect_timeout, Duration::from_secs(10));
    assert_eq!(defaults.request_timeout, Duration::from_secs(300));
    assert_eq!(defaults.gateway_max_in_flight, 64);
    assert_eq!(defaults.upstream.server_retry_max_attempts, 3);
    assert_eq!(defaults.upstream.continuity_replay_bytes_per_run, 8 * MIB);
    assert_eq!(defaults.upstream.usage_response_max_bytes, 256 * KIB);
    assert_eq!(defaults.upstream.provider_client_cache_max_entries, 256);
    assert!(defaults.validate().is_ok());

    assert!(
        OperationalSettings {
            gateway_max_in_flight: 65,
            ..defaults
        }
        .validate()
        .is_err()
    );
    assert!(
        OperationalSettings {
            upstream_response_limit_bytes: 1024 * 1024 + 1,
            ..defaults
        }
        .validate()
        .is_err()
    );
    assert!(
        OperationalSettings {
            admin_api_window: Duration::from_secs(86_401),
            ..defaults
        }
        .validate()
        .is_err()
    );
    assert!(
        OperationalSettings {
            gateway_key_refill_tokens: 0,
            ..defaults
        }
        .validate()
        .is_err()
    );
    assert!(
        OperationalSettings {
            upstream: UpstreamSettings {
                continuity_replay_bytes_total: 4 * MIB,
                ..defaults.upstream
            },
            ..defaults
        }
        .validate()
        .is_err()
    );
}

#[test]
fn app_dir_is_under_the_users_home_directory() {
    assert_eq!(
        app_dir_from_home(Path::new("/root")),
        PathBuf::from("/root/.exoroute")
    );
    #[cfg(windows)]
    assert_eq!(
        app_dir_from_home(Path::new(r"C:\Users\TRUNG NGHIA")),
        PathBuf::from(r"C:\Users\TRUNG NGHIA\.exoroute")
    );
}

#[test]
fn relative_data_paths_are_resolved_from_the_app_directory() {
    let app_dir = env::temp_dir().join(format!("exoroute-paths-{}", uuid::Uuid::new_v4()));
    assert_eq!(
        resolve_path(&app_dir, "data".to_owned()),
        app_dir.join("data")
    );
    let absolute_path = env::temp_dir().join("exoroute-override.lmdb");
    assert_eq!(
        resolve_path(&app_dir, absolute_path.to_string_lossy().into_owned()),
        absolute_path
    );
}

#[test]
fn timeout_values_are_bounded_and_nonzero() {
    assert_eq!(
        checked_duration_ms("CONNECT_TIMEOUT_MS", 100, 100, 120_000),
        Ok(Duration::from_millis(100))
    );
    assert_eq!(
        checked_duration_ms("REQUEST_TIMEOUT_MS", 86_400_000, 100, 86_400_000),
        Ok(Duration::from_secs(86_400))
    );
    assert!(checked_duration_ms("CONNECT_TIMEOUT_MS", 0, 100, 120_000).is_err());
    assert!(checked_duration_ms("REQUEST_TIMEOUT_MS", u64::MAX, 100, 86_400_000).is_err());
}

#[test]
fn initialize_app_dir_creates_a_starter_env_without_overwriting_it() {
    let app_dir = env::temp_dir().join(format!("exoroute-config-{}", uuid::Uuid::new_v4()));
    initialize_app_dir(&app_dir).expect("initialize app directory");
    let env_path = app_dir.join(".env");
    assert!(env_path.is_file());
    assert_eq!(
        fs::read(&env_path).expect("read initial env").as_slice(),
        include_bytes!("../../.env.example").as_slice()
    );

    fs::write(&env_path, b"EXOROUTE_ADMIN_KEY=keep-me\n").expect("write customized env");
    initialize_app_dir(&app_dir).expect("reinitialize app directory");
    assert_eq!(
        fs::read(&env_path).expect("read customized env"),
        b"EXOROUTE_ADMIN_KEY=keep-me\n"
    );
    fs::remove_dir_all(app_dir).expect("remove temporary app directory");
}

#[test]
fn remove_bootstrap_password_preserves_other_environment_settings() {
    let app_dir = env::temp_dir().join(format!("exoroute-env-remove-{}", uuid::Uuid::new_v4()));
    initialize_app_dir(&app_dir).expect("initialize app directory");
    let env_path = app_dir.join(".env");
    fs::write(
        &env_path,
        b"EXOROUTE_ADMIN_KEY=temporary\nEXOROUTE_PORT=8686\nEXOROUTE_MASTER_KEY=keep\n",
    )
    .expect("write environment file");
    remove_env_value(&env_path, "EXOROUTE_ADMIN_KEY").expect("remove bootstrap password");
    assert_eq!(
        fs::read(&env_path).expect("read rewritten environment file"),
        b"EXOROUTE_PORT=8686\nEXOROUTE_MASTER_KEY=keep\n"
    );
    fs::remove_dir_all(app_dir).expect("remove temporary app directory");
}

#[cfg(unix)]
#[test]
fn initialize_refuses_a_symlinked_environment_file() {
    use std::os::unix::fs::symlink;

    let app_dir = env::temp_dir().join(format!("exoroute-env-link-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&app_dir).expect("create app directory");
    let target = app_dir.join("outside.env");
    fs::write(&target, b"EXOROUTE_ADMIN_KEY=keep\n").expect("create target file");
    symlink(&target, app_dir.join(".env")).expect("create env symlink");
    assert!(initialize_app_dir(&app_dir).is_err());
    assert_eq!(
        fs::read(&target).expect("read target file"),
        b"EXOROUTE_ADMIN_KEY=keep\n"
    );
    fs::remove_dir_all(app_dir).expect("remove temporary app directory");
}

#[test]
fn missing_admin_key_is_generated_and_persisted() {
    let app_dir =
        env::temp_dir().join(format!("exoroute-admin-bootstrap-{}", uuid::Uuid::new_v4()));
    initialize_app_dir(&app_dir).expect("initialize app directory");
    let env_path = app_dir.join(".env");

    let (first, generated) = ensure_admin_key(&env_path, None).expect("generate admin key");
    assert!(generated);
    assert!(first.len() >= 32);
    assert!(!crate::admin::password_needs_change(&first));
    assert!(
        fs::read_to_string(&env_path)
            .expect("read env")
            .contains(&format!("EXOROUTE_ADMIN_KEY=\"{first}\""))
    );

    let (second, generated) =
        ensure_admin_key(&env_path, Some(first.clone())).expect("retain configured admin key");
    assert!(!generated);
    assert_eq!(second, first);
    fs::remove_dir_all(app_dir).expect("remove temporary app directory");
}

#[test]
fn missing_master_key_is_generated_persisted_and_stable() {
    let app_dir = env::temp_dir().join(format!("exoroute-master-key-{}", uuid::Uuid::new_v4()));
    initialize_app_dir(&app_dir).expect("initialize app directory");
    let env_path = app_dir.join(".env");

    let (first, generated) = ensure_master_key(&env_path, None).expect("generate master key");
    assert!(generated);
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, first);
    let contents = fs::read_to_string(&env_path).expect("read generated master key");
    assert!(contents.contains(&format!("EXOROUTE_MASTER_KEY=\"{encoded}\"")));
    assert_eq!(
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &encoded)
            .expect("decode generated master key"),
        first
    );

    let (second, generated) =
        ensure_master_key(&env_path, Some(first)).expect("reuse configured master key");
    assert!(!generated);
    assert_eq!(second, first);
    fs::remove_dir_all(app_dir).expect("remove temporary app directory");
}
