use axum::{
    body::Body,
    http::{HeaderName, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

// Make Cargo rebuild the RustEmbed snapshot whenever the frontend build changes.
const _: &str = env!("EXOROUTE_DASHBOARD_ASSET_DIGEST");

#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct DashboardAssets;

pub async fn serve(uri: Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    if requested == "v1"
        || requested.starts_with("v1/")
        || requested == "api"
        || requested.starts_with("api/")
    {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"error":{"message":"API endpoint not found"}})),
        )
            .into_response();
    }
    let resolved_path = if DashboardAssets::get(requested).is_some() {
        requested
    } else if requested.is_empty() || !requested.starts_with("assets/") {
        "index.html"
    } else {
        requested
    };
    let asset = DashboardAssets::get(resolved_path);
    let Some(asset) = asset else {
        return (
            StatusCode::NOT_FOUND,
            "Dashboard assets are missing. Build the web project first.",
        )
            .into_response();
    };
    let content_type = mime_guess::from_path(resolved_path)
        .first_or_octet_stream()
        .essence_str()
        .to_owned();
    let mut response = Response::new(Body::from(asset.data.into_owned()));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if requested.trim_end_matches('/') == "callback" {
            "no-store"
        } else if resolved_path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        }),
    );
    response.headers_mut().insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    response
}
