//! Software update check: bounded GitHub releases lookup, strict SemVer
//! parsing and comparison, and project-pinned release URL validation.

use super::*;
use std::cmp::Ordering;

const RELEASES_API_URL: &str = "https://api.github.com/repos/nghiaomg/ExoRoute/releases/latest";
const RELEASE_URL_PREFIX: &str = "https://github.com/nghiaomg/ExoRoute/releases/tag/";
const UPDATE_CHECK_RESPONSE_LIMIT_BYTES: usize = 64 * 1024;
const UPDATE_CHECK_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const UPDATE_CHECK_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RELEASE_VERSION_CHARS: usize = 64;
const MAX_RELEASE_URL_CHARS: usize = 512;

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
    html_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Vec<ReleaseIdentifier>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReleaseIdentifier {
    Numeric(u64),
    Text(String),
}

pub(super) async fn check_for_updates(State(state): State<AppState>) -> ApiResult {
    let current_version = env!("CARGO_PKG_VERSION");
    let Some(current) = parse_release_version(current_version) else {
        tracing::error!("the packaged ExoRoute version is not valid SemVer");
        return Ok(update_check_unavailable(current_version));
    };
    let operational = state.operational_settings().settings;
    let (url, client) = match crate::security::egress::provider_client(
        RELEASES_API_URL,
        false,
        operational
            .connect_timeout
            .min(UPDATE_CHECK_CONNECT_TIMEOUT),
        operational
            .request_timeout
            .min(UPDATE_CHECK_REQUEST_TIMEOUT),
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        operational.upstream,
    )
    .await
    {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "software update check could not reach the release service");
            return Ok(update_check_unavailable(current_version));
        }
    };
    let response = match client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "software update check request failed");
            return Ok(update_check_unavailable(current_version));
        }
    };
    if !response.status().is_success() {
        tracing::warn!(
            status = response.status().as_u16(),
            "software update check returned a non-success status"
        );
        return Ok(update_check_unavailable(current_version));
    }
    let body = match crate::provider_adapters::read_limited_response(
        response,
        UPDATE_CHECK_RESPONSE_LIMIT_BYTES,
    )
    .await
    {
        Ok(body) => body,
        Err(error) => {
            tracing::warn!(%error, "software update check response was invalid");
            return Ok(update_check_unavailable(current_version));
        }
    };
    let release = match serde_json::from_slice::<LatestRelease>(&body) {
        Ok(release) => release,
        Err(error) => {
            tracing::warn!(%error, "software update check response was not a release record");
            return Ok(update_check_unavailable(current_version));
        }
    };
    let Some(latest) = parse_release_version(&release.tag_name) else {
        tracing::warn!("software update check returned an invalid release version");
        return Ok(update_check_unavailable(current_version));
    };
    let Some(release_url) = validated_release_url(&release.html_url) else {
        tracing::warn!("software update check returned an invalid release URL");
        return Ok(update_check_unavailable(current_version));
    };
    let latest_version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let update_available = compare_release_versions(&latest, &current) == Ordering::Greater;
    Ok(Json(json!({
        "status": if update_available { "update_available" } else { "up_to_date" },
        "current_version": current_version,
        "latest_version": latest_version,
        "update_available": update_available,
        "release_url": release_url,
    })))
}

fn update_check_unavailable(current_version: &str) -> Json<Value> {
    Json(json!({
        "status": "unavailable",
        "current_version": current_version,
        "latest_version": Value::Null,
        "update_available": false,
        "release_url": Value::Null,
    }))
}

fn parse_release_version(value: &str) -> Option<ReleaseVersion> {
    if value.is_empty()
        || value.chars().count() > MAX_RELEASE_VERSION_CHARS
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    let value = value.strip_prefix('v').unwrap_or(value);
    let (value, build) = match value.split_once('+') {
        Some((_, "")) => return None,
        Some((value, build)) => (value, build),
        None => (value, ""),
    };
    if !build.is_empty()
        && (build
            .split('.')
            .any(|identifier| !valid_release_identifier(identifier, false)))
    {
        return None;
    }
    let (core, prerelease) = match value.split_once('-') {
        Some((_, "")) => return None,
        Some((core, prerelease)) => (core, prerelease),
        None => (value, ""),
    };
    let mut core_parts = core.split('.');
    let major = parse_release_number(core_parts.next()?)?;
    let minor = parse_release_number(core_parts.next()?)?;
    let patch = parse_release_number(core_parts.next()?)?;
    if core_parts.next().is_some() {
        return None;
    }
    let prerelease = if prerelease.is_empty() {
        Vec::new()
    } else {
        let mut identifiers = Vec::new();
        for identifier in prerelease.split('.') {
            if !valid_release_identifier(identifier, true) {
                return None;
            }
            if identifier.bytes().all(|byte| byte.is_ascii_digit()) {
                identifiers.push(ReleaseIdentifier::Numeric(identifier.parse().ok()?));
            } else {
                identifiers.push(ReleaseIdentifier::Text(identifier.to_owned()));
            }
        }
        identifiers
    };
    Some(ReleaseVersion {
        major,
        minor,
        patch,
        prerelease,
    })
}

fn parse_release_number(value: &str) -> Option<u64> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

fn valid_release_identifier(value: &str, reject_numeric_leading_zero: bool) -> bool {
    !value.is_empty()
        && (!reject_numeric_leading_zero
            || value.len() == 1
            || !value.starts_with('0')
            || !value.bytes().all(|byte| byte.is_ascii_digit()))
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn compare_release_versions(left: &ReleaseVersion, right: &ReleaseVersion) -> Ordering {
    left.major
        .cmp(&right.major)
        .then_with(|| left.minor.cmp(&right.minor))
        .then_with(|| left.patch.cmp(&right.patch))
        .then_with(|| compare_prerelease(&left.prerelease, &right.prerelease))
}

fn compare_prerelease(left: &[ReleaseIdentifier], right: &[ReleaseIdentifier]) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    for (left, right) in left.iter().zip(right) {
        let ordering = match (left, right) {
            (ReleaseIdentifier::Numeric(left), ReleaseIdentifier::Numeric(right)) => {
                left.cmp(right)
            }
            (ReleaseIdentifier::Numeric(_), ReleaseIdentifier::Text(_)) => Ordering::Less,
            (ReleaseIdentifier::Text(_), ReleaseIdentifier::Numeric(_)) => Ordering::Greater,
            (ReleaseIdentifier::Text(left), ReleaseIdentifier::Text(right)) => left.cmp(right),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

fn validated_release_url(value: &str) -> Option<String> {
    if value.is_empty() || value.chars().count() > MAX_RELEASE_URL_CHARS {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.query().is_some()
        || url.fragment().is_some()
        || !url
            .path()
            .starts_with(RELEASE_URL_PREFIX.strip_prefix("https://github.com")?)
    {
        return None;
    }
    let tag = url
        .path()
        .strip_prefix("/nghiaomg/ExoRoute/releases/tag/")?;
    if tag.is_empty() || tag.contains('/') {
        return None;
    }
    Some(value.to_owned())
}

#[cfg(test)]
mod update_check_tests {
    use super::*;

    #[test]
    fn release_versions_follow_semver_precedence() {
        let current = parse_release_version("v1.2.3").expect("current version parses");
        let patch = parse_release_version("1.2.4").expect("patch version parses");
        let prerelease = parse_release_version("1.2.5-rc.1").expect("prerelease parses");
        let stable = parse_release_version("1.2.5").expect("stable version parses");

        assert_eq!(
            compare_release_versions(&patch, &current),
            Ordering::Greater
        );
        assert_eq!(
            compare_release_versions(&prerelease, &stable),
            Ordering::Less
        );
        assert_eq!(
            compare_release_versions(&stable, &prerelease),
            Ordering::Greater
        );
    }

    #[test]
    fn invalid_release_versions_are_rejected() {
        for version in [
            "",
            "1.2",
            "1.2.3.4",
            "1.02.3",
            "1.2.3-",
            "1.2.3+",
            "1.2.3-01",
            "1.2.3+build/value",
        ] {
            assert!(parse_release_version(version).is_none(), "{version}");
        }
    }

    #[test]
    fn release_links_are_pinned_to_the_project() {
        assert!(
            validated_release_url("https://github.com/nghiaomg/ExoRoute/releases/tag/v1.2.3")
                .is_some()
        );
        for url in [
            "http://github.com/nghiaomg/ExoRoute/releases/tag/v1.2.3",
            "https://evil.example/nghiaomg/ExoRoute/releases/tag/v1.2.3",
            "https://github.com/other/project/releases/tag/v1.2.3",
            "https://github.com/nghiaomg/ExoRoute/releases/tag/v1.2.3?redirect=evil",
        ] {
            assert!(validated_release_url(url).is_none(), "{url}");
        }
    }
}
