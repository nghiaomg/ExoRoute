use super::*;
use crate::infra::storage::{Field, Record, Table};

pub(super) const MAX_ADMIN_PASSWORD_HASH_BYTES: usize = 1024;
const MIN_ARGON2_MEMORY_KIB: u32 = 19_456;
const MIN_ARGON2_ITERATIONS: u32 = 2;
const MIN_ARGON2_LANES: u32 = 1;
const MAX_ARGON2_MEMORY_KIB: u32 = 65_536;
const MAX_ARGON2_ITERATIONS: u32 = 4;
const MAX_ARGON2_LANES: u32 = 4;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdminLoginInput {
    password: String,
}

pub(super) async fn admin_login(
    State(state): State<AppState>,
    mut request: Request<Body>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let input = parse_auth_json::<AdminLoginInput>(&mut request).await?;
    let _auth_guard = state.admin.admin_auth_lock.try_lock().map_err(|_| {
        fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "Admin authentication is busy; retry shortly.",
        )
    })?;
    let _login_permit = state
        .admin
        .admin_login_in_flight
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "admin login is busy; retry shortly",
            )
        })?;
    if input.password.is_empty() || input.password.chars().count() > 128 {
        return Err(fail(
            StatusCode::UNAUTHORIZED,
            "admin authentication required",
        ));
    }

    let stored = state
        .db
        .read(|transaction| {
            transaction
                .get::<Record>(Table::AdminAuth, "singleton")?
                .map(|record| {
                    Ok::<_, crate::infra::storage::StorageError>((
                        record.text("password_hash")?.to_owned(),
                        record.boolean("must_change_password")?,
                    ))
                })
                .transpose()
        })
        .await
        .map_err(internal)?;

    let must_change_password = if let Some((password_hash, must_change)) = stored {
        if !verify_admin_password_async(password_hash, input.password.clone())
            .await
            .map_err(|_| {
                fail(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "admin authentication is busy",
                )
            })?
        {
            return Err(fail(
                StatusCode::UNAUTHORIZED,
                "admin authentication required",
            ));
        }
        must_change || password_needs_change(&input.password)
    } else {
        let configured_password = state.admin_key.read().await.clone();
        let Some(configured_password) = configured_password else {
            return Err(fail(
                StatusCode::UNAUTHORIZED,
                "admin authentication is not configured",
            ));
        };
        if !secure_eq(configured_password.as_bytes(), input.password.as_bytes()) {
            return Err(fail(
                StatusCode::UNAUTHORIZED,
                "admin authentication required",
            ));
        }

        let password_hash = hash_admin_password_async(input.password.clone())
            .await
            .map_err(|_| {
                fail(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "could not initialize admin authentication",
                )
            })?;
        let forced_change = password_needs_change(&input.password);
        let bootstrap_record = Record::new()
            .with("password_hash", Field::Text(password_hash))
            .with("must_change_password", Field::Bool(forced_change))
            .with(
                "updated_at",
                Field::Text(crate::infra::db::utc_timestamp_now().map_err(internal)?),
            );
        state
            .db
            .write(move |transaction| {
                if transaction
                    .get::<Record>(Table::AdminAuth, "singleton")?
                    .is_none()
                {
                    transaction.put(Table::AdminAuth, "singleton", &bootstrap_record)?;
                }
                Ok(())
            })
            .await
            .map_err(internal)?;

        let (canonical_hash, canonical_must_change) = state
            .db
            .read(|transaction| {
                let record = transaction
                    .get::<Record>(Table::AdminAuth, "singleton")?
                    .ok_or_else(|| {
                        crate::infra::storage::StorageError::Invalid(
                            "admin authentication record is missing after initialization"
                                .to_owned(),
                        )
                    })?;
                Ok((
                    record.text("password_hash")?.to_owned(),
                    record.boolean("must_change_password")?,
                ))
            })
            .await
            .map_err(internal)?;
        if !verify_admin_password_async(canonical_hash, input.password.clone())
            .await
            .map_err(|_| {
                fail(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "admin authentication is busy",
                )
            })?
        {
            return Err(fail(
                StatusCode::UNAUTHORIZED,
                "admin authentication required",
            ));
        }
        canonical_must_change
    };

    state
        .admin
        .admin_password_change_required
        .store(must_change_password, std::sync::atomic::Ordering::Release);
    let session = super::sessions::issue_admin_session(&state, must_change_password)
        .await
        .map_err(super::sessions::session_database_error)?;
    let mut response = Json(json!({
        "authenticated": true,
        "access_token": session.access_token,
        "access_expires_in_seconds": super::sessions::ACCESS_TOKEN_TTL_SECONDS,
        "must_change_password": must_change_password
    }))
    .into_response();
    super::cookie_policy::set_refresh_cookie(&mut response, &state, &request, &session)?;
    Ok(response)
}

pub(super) async fn parse_auth_json<T: serde::de::DeserializeOwned>(
    request: &mut Request<Body>,
) -> Result<T, (StatusCode, Json<Value>)> {
    const MAX_AUTH_JSON_BYTES: usize = 8 * 1024;
    let body = std::mem::replace(request.body_mut(), Body::empty());
    let bytes = axum::body::to_bytes(body, MAX_AUTH_JSON_BYTES)
        .await
        .map_err(|_| {
            fail(
                StatusCode::BAD_REQUEST,
                "invalid authentication request body",
            )
        })?;
    serde_json::from_slice(&bytes).map_err(|_| {
        fail(
            StatusCode::BAD_REQUEST,
            "invalid authentication request body",
        )
    })
}

pub(crate) fn hash_admin_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| error.to_string())
}

pub(super) fn verify_admin_password(encoded_hash: &str, password: &str) -> bool {
    if !is_supported_admin_password_hash(encoded_hash) {
        return false;
    }
    let Ok(hash) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &hash)
        .is_ok()
}

pub(super) fn is_supported_admin_password_hash(encoded_hash: &str) -> bool {
    if encoded_hash.len() > MAX_ADMIN_PASSWORD_HASH_BYTES {
        return false;
    }
    let Ok(hash) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    let Ok(params) = argon2::Params::try_from(&hash) else {
        return false;
    };
    hash.algorithm.as_str() == "argon2id"
        && params.m_cost() >= MIN_ARGON2_MEMORY_KIB
        && params.t_cost() >= MIN_ARGON2_ITERATIONS
        && params.p_cost() >= MIN_ARGON2_LANES
        && params.m_cost() <= MAX_ARGON2_MEMORY_KIB
        && params.t_cost() <= MAX_ARGON2_ITERATIONS
        && params.p_cost() <= MAX_ARGON2_LANES
}

async fn hash_admin_password_async(password: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || hash_admin_password(&password))
        .await
        .map_err(|error| format!("password hashing task failed: {error}"))?
}

pub(super) async fn verify_admin_password_async(
    encoded_hash: String,
    password: String,
) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || verify_admin_password(&encoded_hash, &password))
        .await
        .map_err(|error| format!("password verification task failed: {error}"))
}

pub(crate) fn password_needs_change(password: &str) -> bool {
    let normalized = password.trim().to_ascii_lowercase();
    let chars: Vec<char> = normalized.chars().collect();
    let repeated = chars
        .first()
        .is_some_and(|first| chars.iter().all(|c| c == first));
    chars.len() < 12 || repeated || zxcvbn::zxcvbn(password, &[]).score() < zxcvbn::Score::Three
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdminPasswordInput {
    #[serde(default)]
    current_password: String,
    new_password: String,
}

pub(super) async fn change_admin_password(
    State(state): State<AppState>,
    mut request: Request<Body>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let input = parse_auth_json::<AdminPasswordInput>(&mut request).await?;
    let _auth_guard = state.admin.admin_auth_lock.try_lock().map_err(|_| {
        fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "Admin authentication is busy; retry shortly.",
        )
    })?;
    let _password_work_permit = state
        .admin
        .admin_login_in_flight
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "admin password service is busy; retry shortly",
            )
        })?;
    let password = input.new_password;
    let password_length = password.chars().count();
    if !(12..=128).contains(&password_length)
        || password.trim().is_empty()
        || password_needs_change(&password)
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "new password must be at least 12 characters and score at least 3 on the strength check",
        ));
    }
    if password.chars().any(char::is_control) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "new password cannot contain control characters",
        ));
    }

    let presented_session = request
        .headers()
        .get("authorization")
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "))
        .or_else(|| {
            request
                .headers()
                .get("x-admin-key")
                .and_then(|header| header.to_str().ok())
        });
    let forced_session = match presented_session {
        Some(token) => state
            .admin_session(token)
            .await
            .is_some_and(|session| session.must_change_password),
        None => false,
    };
    if !forced_session {
        let current_auth = state
            .db
            .read(|transaction| {
                transaction
                    .get::<Record>(Table::AdminAuth, "singleton")?
                    .map(|record| record.text("password_hash").map(str::to_owned))
                    .transpose()
            })
            .await
            .map_err(internal)?;
        let current_is_valid = if let Some(encoded_hash) = current_auth {
            verify_admin_password_async(encoded_hash, input.current_password.clone())
                .await
                .map_err(|_| {
                    fail(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "admin authentication is busy",
                    )
                })?
        } else {
            state
                .admin_key
                .read()
                .await
                .as_deref()
                .is_some_and(|expected| {
                    secure_eq(expected.as_bytes(), input.current_password.as_bytes())
                })
        };
        if !current_is_valid {
            return Err(fail(
                StatusCode::UNAUTHORIZED,
                "current password is incorrect",
            ));
        }
    }
    if secure_eq(password.as_bytes(), input.current_password.as_bytes()) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "new password must be different from the current password",
        ));
    }

    let password_hash = hash_admin_password_async(password.clone())
        .await
        .map_err(|_| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not save the admin password",
            )
        })?;
    let updated_at = crate::infra::db::utc_timestamp_now().map_err(internal)?;
    let auth_record = Record::new()
        .with("password_hash", Field::Text(password_hash))
        .with("must_change_password", Field::Bool(false))
        .with("updated_at", Field::Text(updated_at));
    state
        .db
        .write(move |transaction| {
            transaction.put(Table::AdminAuth, "singleton", &auth_record)?;
            super::sessions::revoke_all_sessions_in_transaction(transaction)
        })
        .await
        .map_err(internal)?;

    state
        .admin
        .admin_password_change_required
        .store(false, std::sync::atomic::Ordering::Release);
    state.admin_key.write().await.take();
    let env_file = state.config.app_dir.join(".env");
    if state.config.app_dir.is_absolute()
        && env_file.is_file()
        && let Err(error) = crate::config::remove_env_value(&env_file, "EXOROUTE_ADMIN_KEY")
    {
        tracing::warn!(%error, "could not remove the obsolete bootstrap admin password from .env");
    }
    state.clear_admin_sessions().await;
    let session = super::sessions::issue_admin_session(&state, false)
        .await
        .map_err(super::sessions::session_database_error)?;
    let mut response = Json(json!({
        "ok":true,
        "admin_password_configured":true,
        "must_change_password":false,
        "access_token":session.access_token,
        "access_expires_in_seconds":super::sessions::ACCESS_TOKEN_TTL_SECONDS
    }))
    .into_response();
    super::cookie_policy::set_refresh_cookie(&mut response, &state, &request, &session)?;
    Ok(response)
}
