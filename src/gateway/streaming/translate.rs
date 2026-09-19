use super::*;

#[path = "translate_driver.rs"]
mod driver;
use driver::translation_stream;

#[cfg(test)]
pub(crate) fn stream_translation(
    upstream: reqwest::Response,
    config: StreamTranslationConfig,
) -> Response {
    stream_translation_from_chunks(Box::pin(upstream.bytes_stream()), config)
}

pub(crate) fn stream_translation_from_chunks(
    upstream: UpstreamChunkStream,
    config: StreamTranslationConfig,
) -> Response {
    let request_id = config.request_id.clone();
    let stream = translation_stream(upstream, config);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    response.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&request_id).unwrap_or_else(|_| HeaderValue::from_static("unknown")),
    );
    response
}
