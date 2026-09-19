//! Admin browser policy: refresh-cookie naming and parsing, Secure/SameSite
//! attributes, trusted forwarded-scheme handling, and same-origin POST checks.
//! Pure HTTP/browser policy; session persistence lives in `sessions.rs`.

use super::sessions::IssuedAdminSession;
use super::*;
use axum::extract::ConnectInfo;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use http::uri::Authority;
use sha2::{Digest, Sha256};
use std::str::FromStr;

pub(super) const REFRESH_COOKIE_PATH: &str = "/api/v1/admin/auth";

pub(super) fn cookie_name(state: &AppState) -> String {
    let mut hasher = Sha256::new();
    hasher.update(
        state
            .config
            .app_dir
            .as_os_str()
            .to_string_lossy()
            .as_bytes(),
    );
    hasher.update([0]);
    hasher.update(
        state
            .config
            .database_path
            .as_os_str()
            .to_string_lossy()
            .as_bytes(),
    );
    let digest = hasher.finalize();
    let suffix = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("exoroute_refresh_{suffix}")
}

pub(super) fn refresh_cookie(headers: &HeaderMap, state: &AppState) -> Option<String> {
    let name = cookie_name(state);
    let mut result = None;
    let mut header_bytes = 0_usize;
    for header_value in headers.get_all(header::COOKIE).iter() {
        let value = header_value.to_str().ok()?;
        header_bytes = header_bytes.saturating_add(value.len());
        if header_bytes > 8192 {
            return None;
        }
        for pair in value.split(';') {
            let Some((key, value)) = pair.trim().split_once('=') else {
                continue;
            };
            if key.trim() == name {
                if result.is_some() || !is_valid_token(value.trim()) {
                    return None;
                }
                result = Some(value.trim().to_owned());
            }
        }
    }
    result
}

fn is_valid_token(token: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(token)
        .is_ok_and(|bytes| bytes.len() == 32)
}

pub(super) fn cookie_is_secure(state: &AppState, request: &Request<Body>) -> bool {
    trusted_forwarded_scheme(state, request).as_deref() == Some("https")
}

fn trusted_forwarded_scheme(state: &AppState, request: &Request<Body>) -> Option<String> {
    let peer_is_trusted = state.config.trust_proxy_headers
        && request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .is_some_and(|ConnectInfo(address)| address.ip().is_loopback());
    if !peer_is_trusted {
        return Some("http".to_owned());
    }
    let forwarded = request.headers().get_all("x-forwarded-proto");
    if forwarded.iter().count() == 0 {
        return Some("http".to_owned());
    }
    if forwarded.iter().count() != 1 {
        return None;
    }
    match forwarded.iter().next()?.to_str().ok()?.trim() {
        "http" => Some("http".to_owned()),
        "https" => Some("https".to_owned()),
        _ => None,
    }
}

pub(super) fn same_origin_browser_post(state: &AppState, request: &Request<Body>) -> bool {
    let origins = request.headers().get_all(header::ORIGIN);
    let fetch_sites = request.headers().get_all("sec-fetch-site");
    if origins.iter().count() != 1 || fetch_sites.iter().count() != 1 {
        return false;
    }
    if fetch_sites
        .iter()
        .next()
        .and_then(|value| value.to_str().ok())
        .is_none_or(|value| value != "same-origin")
    {
        return false;
    }
    let Some(scheme) = trusted_forwarded_scheme(state, request) else {
        return false;
    };
    let hosts = request.headers().get_all(header::HOST);
    if hosts.iter().count() != 1 {
        return false;
    }
    let Some(host) = hosts.iter().next().and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Some(origin) = origins.iter().next().and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Some((origin_scheme, origin_authority)) = origin.split_once("://") else {
        return false;
    };
    if origin_scheme != scheme
        || origin_authority.is_empty()
        || origin_authority.contains('@')
        || origin_authority
            .chars()
            .any(|character| matches!(character, '/' | '?' | '#'))
    {
        return false;
    }
    normalized_authority(origin_authority, &scheme)
        .zip(normalized_authority(host, &scheme))
        .is_some_and(|(origin, host)| origin == host)
}

fn normalized_authority(value: &str, scheme: &str) -> Option<(String, u16)> {
    if value
        .chars()
        .any(|character| character.is_control() || matches!(character, '@' | ',' | '\\'))
    {
        return None;
    }
    let authority = Authority::from_str(value).ok()?;
    let host = authority.host().to_ascii_lowercase();
    let port = authority.port_u16().or(match scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    })?;
    Some((host, port))
}

pub(super) fn set_refresh_cookie(
    response: &mut Response,
    state: &AppState,
    request: &Request<Body>,
    session: &IssuedAdminSession,
) -> Result<(), (StatusCode, Json<Value>)> {
    let secure = if cookie_is_secure(state, request) {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "{}={}; Path={}; Max-Age={}; HttpOnly; SameSite=Strict{}",
        cookie_name(state),
        session.refresh_token,
        REFRESH_COOKIE_PATH,
        session.refresh_max_age,
        secure
    );
    let value = HeaderValue::from_str(&cookie).map_err(|_| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not create admin session",
        )
    })?;
    response.headers_mut().insert(header::SET_COOKIE, value);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(())
}

pub(super) fn clear_refresh_cookie(
    response: &mut Response,
    state: &AppState,
    request: &Request<Body>,
) -> Result<(), (StatusCode, Json<Value>)> {
    clear_refresh_cookie_for_scheme(response, state, cookie_is_secure(state, request))
}

pub(super) fn clear_refresh_cookie_for_scheme(
    response: &mut Response,
    state: &AppState,
    secure: bool,
) -> Result<(), (StatusCode, Json<Value>)> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{}=; Path={}; Max-Age=0; HttpOnly; SameSite=Strict{}",
        cookie_name(state),
        REFRESH_COOKIE_PATH,
        secure
    );
    let value = HeaderValue::from_str(&cookie).map_err(|_| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not clear admin session",
        )
    })?;
    response.headers_mut().insert(header::SET_COOKIE, value);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(())
}
