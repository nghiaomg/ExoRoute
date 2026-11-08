//! Import of Antigravity CLI (`agy`) credentials.
//!
//! The `agy` CLI stores its Google OAuth credential at
//! `~/.gemini/antigravity-cli/antigravity-oauth-token` (or the shared
//! `~/.gemini/oauth_creds.json` cache). Both files carry an access token and a
//! refresh token; importing one lets an operator reuse an existing CLI login
//! instead of running the browser OAuth flow again. Imported accounts are
//! stored with the `cli` client profile so refresh can use CLI-specific OAuth
//! client credentials when they differ from the bundled desktop client.
use super::{
    AntigravityAccount, MAX_TOKEN_BYTES, bounded_text, bounded_token, enrich_account, save_account,
    unix_now,
};
use crate::state::AppState;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

pub(super) const PROFILE_CLI: &str = "cli";
pub const CLI_CLIENT_ID_ENV: &str = "EXOROUTE_ANTIGRAVITY_CLI_OAUTH_CLIENT_ID";
pub const CLI_CLIENT_SECRET_ENV: &str = "EXOROUTE_ANTIGRAVITY_CLI_OAUTH_CLIENT_SECRET";

const MAX_CLI_IMPORT_BYTES: usize = 256 * 1024;
const MAX_CLI_FILE_BYTES: u64 = MAX_CLI_IMPORT_BYTES as u64;

/// Credential files the `agy` CLI (and the shared Gemini CLI store) may use,
/// in detection order.
fn cli_credential_paths() -> Result<Vec<PathBuf>, String> {
    let home = home_dir()?;
    Ok(vec![
        home.join(".gemini")
            .join("antigravity-cli")
            .join("antigravity-oauth-token"),
        home.join(".gemini").join("oauth_creds.json"),
    ])
}

fn home_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");
    home.map(PathBuf::from).ok_or_else(|| {
        "could not find the user home directory (HOME/USERPROFILE is not set)".to_owned()
    })
}

/// Parses an `agy`/Gemini CLI credential document into an account. Accepts the
/// `agy` token-file envelope (`{"token": {...}}`), a flat token response, and
/// the Gemini CLI `oauth_creds.json` shape — all reduce to the same fields.
pub(super) fn parse_cli_credential(content: &str) -> Result<AntigravityAccount, String> {
    if content.len() > MAX_CLI_IMPORT_BYTES {
        return Err("Antigravity CLI credential file is too large".to_owned());
    }
    let value: Value = serde_json::from_str(content)
        .map_err(|_| "Antigravity CLI credential is not valid JSON".to_owned())?;
    let token = value
        .get("token")
        .filter(|token| token.is_object())
        .unwrap_or(&value);
    let access_token = bounded_token(
        token
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or("Antigravity CLI credential has no access token")?,
    )?;
    let refresh_token = token
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(bounded_token)
        .transpose()?;
    if refresh_token.is_none() {
        return Err(
            "Antigravity CLI credential has no refresh token; sign in with agy again so it can renew access tokens"
                .to_owned(),
        );
    }
    let expires_at = token_expiry(token).unwrap_or(0);
    Ok(AntigravityAccount {
        access_token,
        refresh_token,
        expires_at,
        scope: token.get("scope").and_then(Value::as_str).map(bounded_text),
        email: None,
        project_id: None,
        tier: None,
        client_profile: PROFILE_CLI.to_owned(),
    })
}

fn token_expiry(token: &Value) -> Option<i64> {
    for key in ["expires_at", "expiry", "expiry_date"] {
        if let Some(expiry) = token.get(key).and_then(parse_expiry_value) {
            return Some(expiry);
        }
    }
    token
        .get("expires_in")
        .and_then(Value::as_i64)
        .map(|seconds| unix_now().saturating_add(seconds.clamp(0, 31_536_000)))
}

fn parse_expiry_value(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        // Chrome-style `expiry_date` is milliseconds; second-precision values
        // are already near the current epoch.
        let seconds = if number.abs() > 100_000_000_000 {
            number / 1000
        } else {
            number
        };
        return Some(seconds);
    }
    let text = value.as_str()?.trim();
    if let Ok(epoch) = text.parse::<i64>() {
        return Some(if epoch.abs() > 100_000_000_000 {
            epoch / 1000
        } else {
            epoch
        });
    }
    parse_rfc3339_epoch(text)
}

/// Minimal RFC 3339 timestamp parser (`YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)`).
/// Returns Unix seconds; sub-second precision is truncated.
fn parse_rfc3339_epoch(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 {
        return None;
    }
    let year = parse_digits(bytes, 0, 4)? as i64;
    if bytes.get(4) != Some(&b'-') {
        return None;
    }
    let month = parse_digits(bytes, 5, 2)? as i64;
    if bytes.get(7) != Some(&b'-') {
        return None;
    }
    let day = parse_digits(bytes, 8, 2)? as i64;
    if bytes.get(10) != Some(&b'T') && bytes.get(10) != Some(&b't') {
        return None;
    }
    let hour = parse_digits(bytes, 11, 2)? as i64;
    if bytes.get(13) != Some(&b':') {
        return None;
    }
    let minute = parse_digits(bytes, 14, 2)? as i64;
    if bytes.get(16) != Some(&b':') {
        return None;
    }
    let second = parse_digits(bytes, 17, 2)? as i64;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Days since epoch via civil calendar conversion (Howard Hinnant's algorithm).
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let mut epoch = days * 86_400 + hour * 3_600 + minute * 60 + second;

    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        while bytes.get(index).is_some_and(|byte| byte.is_ascii_digit()) {
            index += 1;
        }
    }
    let offset_seconds = match bytes.get(index) {
        Some(b'Z') | Some(b'z') => 0,
        Some(b'+') | Some(b'-') => {
            let sign = if bytes[index] == b'-' { -1 } else { 1 };
            let offset_hour = parse_digits(bytes, index + 1, 2)? as i64;
            if bytes.get(index + 3) != Some(&b':') {
                return None;
            }
            let offset_minute = parse_digits(bytes, index + 4, 2)? as i64;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            sign * (offset_hour * 3_600 + offset_minute * 60)
        }
        _ => return None,
    };
    epoch -= offset_seconds;
    Some(epoch)
}

fn parse_digits(bytes: &[u8], offset: usize, len: usize) -> Option<i64> {
    let slice = bytes.get(offset..offset + len)?;
    if !slice.iter().all(u8::is_ascii_digit) {
        return None;
    }
    slice.iter().try_fold(0i64, |acc, byte| {
        acc.checked_mul(10)?.checked_add(i64::from(byte - b'0'))
    })
}

async fn read_cli_file(path: &std::path::Path) -> Option<String> {
    let metadata = tokio::fs::metadata(path).await.ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CLI_FILE_BYTES {
        return None;
    }
    let bytes = tokio::fs::read(path).await.ok()?;
    if bytes.len() as u64 > MAX_CLI_FILE_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Imports an `agy` CLI login for an Antigravity provider. `content` is the
/// pasted token-file JSON; when `None`, the well-known CLI credential paths
/// under the user's home directory are probed in order.
pub async fn import_cli_login(
    state: &AppState,
    provider_id: &str,
    content: Option<&str>,
) -> Result<String, String> {
    let document = match content {
        Some(content) => {
            if content.trim().is_empty() {
                return Err("Antigravity CLI credential content is empty".to_owned());
            }
            content.to_owned()
        }
        None => {
            let mut found = None;
            for path in cli_credential_paths()? {
                if let Some(text) = read_cli_file(&path).await {
                    found = Some(text);
                    break;
                }
            }
            found.ok_or_else(|| {
                "no Antigravity CLI login found on this machine; paste the agy token file contents instead"
                    .to_owned()
            })?
        }
    };
    let mut account = parse_cli_credential(&document)?;
    if account.access_token.len() > MAX_TOKEN_BYTES {
        return Err("Antigravity CLI credential is invalid".to_owned());
    }
    let connect_timeout = state.config.connect_timeout.min(Duration::from_secs(10));
    let request_timeout = state.config.request_timeout;
    let upstream = state.operational_settings().settings.upstream;
    enrich_account(&mut account, connect_timeout, request_timeout, upstream).await?;
    save_account(state, provider_id, account.clone()).await?;
    Ok(account.display_name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_agy_token_file_envelope() {
        let account = parse_cli_credential(
            r#"{"token":{"access_token":"ya29.access","token_type":"Bearer","refresh_token":"1//refresh","expiry":"2026-05-20T17:19:27.000000000Z"},"auth_method":"consumer"}"#,
        )
        .expect("agy token file parses");
        assert_eq!(account.access_token, "ya29.access");
        assert_eq!(account.refresh_token.as_deref(), Some("1//refresh"));
        assert_eq!(account.client_profile, PROFILE_CLI);
        assert_eq!(account.expires_at, 1_779_297_567);
    }

    #[test]
    fn parses_a_flat_gemini_oauth_creds_document() {
        let account = parse_cli_credential(
            r#"{"access_token":"ya29.flat","refresh_token":"1//flat","scope":"cloud-platform","expiry":"2026-01-02T03:04:05Z"}"#,
        )
        .expect("flat credential parses");
        assert_eq!(account.access_token, "ya29.flat");
        assert_eq!(account.scope.as_deref(), Some("cloud-platform"));
        assert_eq!(account.expires_at, 1_767_323_045);
    }

    #[test]
    fn rejects_documents_without_a_refresh_token() {
        let error = parse_cli_credential(r#"{"token":{"access_token":"ya29.only"}}"#)
            .expect_err("refresh token is required");
        assert!(error.contains("refresh token"));
    }

    #[test]
    fn rejects_malformed_json_and_missing_access_token() {
        assert!(parse_cli_credential("not json").is_err());
        assert!(parse_cli_credential(r#"{"refresh_token":"1//x"}"#).is_err());
    }

    #[test]
    fn rfc3339_parser_handles_offsets_and_fractional_seconds() {
        assert_eq!(
            parse_rfc3339_epoch("2026-05-20T17:19:27.123456789Z"),
            Some(1_779_297_567)
        );
        assert_eq!(
            parse_rfc3339_epoch("2026-05-20T19:19:27+02:00"),
            Some(1_779_297_567)
        );
        assert_eq!(
            parse_rfc3339_epoch("2026-05-20T15:19:27-02:00"),
            Some(1_779_297_567)
        );
        assert_eq!(parse_rfc3339_epoch("not-a-date"), None);
        assert_eq!(parse_rfc3339_epoch("2026-13-40T99:99:99Z"), None);
    }
}
