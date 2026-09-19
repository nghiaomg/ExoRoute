//! Safe inlining of remote images for OpenAI Codex Responses requests.
//!
//! Codex accepts image data URLs in Responses input items. This module fetches
//! only explicitly referenced public HTTPS images, using ExoRoute's pinned
//! egress client with redirects disabled, and applies tight size limits before
//! adding any bytes to the JSON request.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use serde_json::Value;

const MAX_IMAGE_URL_BYTES: usize = 2_048;
const USER_AGENT: &str = "ExoRoute/codex-image-fetch";

/// Replaces remote image URLs in `input[*].content[*]` Codex Responses items
/// with data URLs. Existing data URLs and all sibling fields (including image
/// detail settings) are left unchanged.
///
/// No inbound request headers are accepted by this function or forwarded to
/// image hosts. Each fetch uses a pinned public-egress client with redirects
/// disabled and a streamed body cap.
pub async fn inline_remote_images(
    body: &mut Value,
    upstream: crate::config::UpstreamSettings,
) -> Result<(), String> {
    let Some(input) = body.get("input").and_then(Value::as_array) else {
        return Ok(());
    };

    let mut image_urls = Vec::new();
    for (input_index, item) in input.iter().enumerate() {
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for (content_index, block) in content.iter().enumerate() {
            if block.get("type").and_then(Value::as_str) != Some("input_image") {
                continue;
            }
            let Some(url) = block.get("image_url").and_then(Value::as_str) else {
                continue;
            };
            if is_data_uri(url) {
                continue;
            }
            validate_remote_image_url(url)?;
            if image_urls.len() >= upstream.remote_image_max_count {
                return Err(format!(
                    "Codex image count exceeds the limit of {} remote images",
                    upstream.remote_image_max_count
                ));
            }
            image_urls.push((input_index, content_index, url.to_owned()));
        }
    }

    let mut total_bytes = 0usize;
    for (input_index, content_index, url) in image_urls {
        let remaining = upstream
            .remote_image_total_max_bytes
            .saturating_sub(total_bytes);
        if remaining == 0 {
            return Err(format!(
                "Codex remote images exceed the aggregate limit of {} bytes",
                upstream.remote_image_total_max_bytes
            ));
        }
        let limit = upstream.remote_image_max_bytes.min(remaining);
        let (mime, bytes) = fetch_remote_image(&url, limit, upstream).await?;
        total_bytes = total_bytes.saturating_add(bytes.len());
        let data_url = format!("data:{mime};base64,{}", STANDARD.encode(&bytes));
        body["input"][input_index]["content"][content_index]["image_url"] = Value::String(data_url);
    }

    Ok(())
}

fn is_data_uri(value: &str) -> bool {
    value
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
}

fn validate_remote_image_url(value: &str) -> Result<(), String> {
    if value.len() > MAX_IMAGE_URL_BYTES {
        return Err(format!(
            "Codex remote image URL exceeds the limit of {MAX_IMAGE_URL_BYTES} bytes"
        ));
    }
    let url =
        reqwest::Url::parse(value).map_err(|_| "Codex remote image URL is invalid".to_owned())?;
    if url.scheme() != "https" {
        return Err("Codex remote image URLs must use HTTPS".to_owned());
    }
    crate::security::egress::validate_provider_url(value, false)
        .map(|_| ())
        .map_err(|error| format!("Codex remote image URL rejected by egress policy: {error}"))
}

async fn fetch_remote_image(
    value: &str,
    byte_limit: usize,
    upstream: crate::config::UpstreamSettings,
) -> Result<(&'static str, Vec<u8>), String> {
    // Validate again at the fetch boundary, then use the pinned client which
    // performs DNS/IP checks and disables redirects for the actual request.
    validate_remote_image_url(value)?;
    let (url, client) = crate::security::egress::provider_client(
        value,
        false,
        upstream.remote_image_connect_timeout,
        upstream.remote_image_request_timeout,
        USER_AGENT,
        upstream,
    )
    .await
    .map_err(|error| format!("Could not safely resolve Codex remote image: {error}"))?;

    // Deliberately do not copy Authorization, cookies, or any other caller
    // headers onto this request. Redirects are disabled in provider_client.
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "Could not fetch Codex remote image".to_owned())?;
    if !response.status().is_success() {
        return Err(format!(
            "Codex remote image server returned HTTP {}",
            response.status().as_u16()
        ));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let mime = allowed_image_mime(content_type)?;
    if response
        .content_length()
        .is_some_and(|length| length > byte_limit as u64)
    {
        return Err(format!(
            "Codex remote image exceeds the per-image limit of {byte_limit} bytes"
        ));
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Could not read Codex remote image".to_owned())?;
        append_bounded(&mut bytes, &chunk, byte_limit)?;
    }
    if !image_signature_matches(mime, &bytes) {
        return Err("Codex remote image bytes do not match the declared image type".to_owned());
    }
    Ok((mime, bytes))
}

fn allowed_image_mime(content_type: Option<&str>) -> Result<&'static str, String> {
    let mime = content_type
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    if mime.eq_ignore_ascii_case("image/jpeg") {
        Ok("image/jpeg")
    } else if mime.eq_ignore_ascii_case("image/png") {
        Ok("image/png")
    } else if mime.eq_ignore_ascii_case("image/gif") {
        Ok("image/gif")
    } else if mime.eq_ignore_ascii_case("image/webp") {
        Ok("image/webp")
    } else {
        Err("Codex remote image must use JPEG, PNG, GIF, or WebP content type".to_owned())
    }
}

fn append_bounded(target: &mut Vec<u8>, chunk: &[u8], byte_limit: usize) -> Result<(), String> {
    if target.len().saturating_add(chunk.len()) > byte_limit {
        return Err(format!(
            "Codex remote image exceeds the per-image limit of {byte_limit} bytes"
        ));
    }
    target.extend_from_slice(chunk);
    Ok(())
}

fn image_signature_matches(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn preserves_existing_data_uris_and_image_detail() {
        let mut body = json!({
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_image",
                    "image_url": "data:image/png;base64,aGVsbG8=",
                    "detail": "high"
                }]
            }]
        });
        let original = body.clone();

        inline_remote_images(&mut body, crate::config::UpstreamSettings::default())
            .await
            .unwrap();

        assert_eq!(body, original);
    }

    #[tokio::test]
    async fn rejects_non_https_schemes_without_fetching() {
        let mut body = json!({
            "input": [{"content": [{"type": "input_image", "image_url": "ftp://example.com/image.png"}]}]
        });

        let error = inline_remote_images(&mut body, crate::config::UpstreamSettings::default())
            .await
            .unwrap_err();

        assert!(error.contains("must use HTTPS"));
        assert_eq!(
            body["input"][0]["content"][0]["image_url"],
            "ftp://example.com/image.png"
        );
    }

    #[test]
    fn rejects_private_and_credential_bearing_urls() {
        assert!(validate_remote_image_url("https://127.0.0.1/image.png").is_err());
        assert!(validate_remote_image_url("https://user:secret@example.com/image.png").is_err());
    }

    #[test]
    fn restricts_mime_types_and_checks_image_signatures() {
        assert_eq!(
            allowed_image_mime(Some("Image/PNG; charset=binary")).unwrap(),
            "image/png"
        );
        assert!(allowed_image_mime(Some("image/svg+xml")).is_err());
        assert!(image_signature_matches(
            "image/png",
            b"\x89PNG\r\n\x1a\nimage"
        ));
        assert!(!image_signature_matches("image/png", b"<svg></svg>"));
    }

    #[test]
    fn enforces_streamed_byte_limit() {
        let mut bytes = vec![1, 2, 3];
        assert!(append_bounded(&mut bytes, &[4, 5], 4).is_err());
        assert_eq!(bytes, [1, 2, 3]);
    }
}
