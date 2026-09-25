//! Shared mock upstream servers for tests that drive real HTTP framing.
//!
//! Every helper binds an ephemeral loopback port, so a test never depends on a
//! fixed port and never sends traffic outside the loopback interface.

use axum::{
    Router,
    body::Body,
    http::{StatusCode, header},
    response::Response,
};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};

/// Binds `router` to an ephemeral loopback port and serves it for the lifetime
/// of the test, returning the address the gateway should call.
pub(crate) async fn spawn_upstream(router: Router<()>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("mock upstream server");
    });
    address
}

/// A `text/event-stream` success response for an axum mock upstream handler.
pub(crate) fn sse_response(body: impl Into<Vec<u8>>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(body.into()))
        .expect("mock upstream SSE response")
}

/// The request head a scripted mock upstream received.
pub(crate) struct MockRequest {
    pub(crate) request_line: String,
    pub(crate) authorization: Option<String>,
}

/// One scripted HTTP/1.1 response served by [`spawn_mock_http_server`].
pub(crate) struct MockResponse {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    delay_before_body: Option<Duration>,
    hold_after_body: Option<Duration>,
}

impl MockResponse {
    /// A JSON response carrying `body`.
    pub(crate) fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self::new(status, "application/json", body.into())
    }

    /// A `text/event-stream` success response carrying `body`.
    pub(crate) fn sse(body: impl Into<Vec<u8>>) -> Self {
        Self::new(200, "text/event-stream", body.into())
    }

    fn new(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            body,
            delay_before_body: None,
            hold_after_body: None,
        }
    }

    /// Waits after the response headers so the client observes a late body.
    pub(crate) fn delay_before_body(mut self, delay: Duration) -> Self {
        self.delay_before_body = Some(delay);
        self
    }

    /// Keeps the connection open and silent after the body instead of closing
    /// it, so the client observes a transport idle timeout.
    pub(crate) fn hold_after_body(mut self, hold: Duration) -> Self {
        self.hold_after_body = Some(hold);
        self
    }
}

/// Serves `responses` in order on an ephemeral loopback port, returning the
/// bound address and a handle to the request heads it received.
pub(crate) async fn spawn_mock_http_server(
    responses: Vec<MockResponse>,
) -> (SocketAddr, JoinHandle<Vec<MockRequest>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock HTTP upstream listener");
    let address = listener.local_addr().expect("mock HTTP upstream address");
    let server = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(responses.len());
        for response in responses {
            let (stream, _) = listener.accept().await.expect("mock HTTP request");
            let mut stream = BufReader::new(stream);
            requests.push(read_request_head(&mut stream).await);
            write_response(&mut stream, &response).await;
        }
        requests
    });
    (address, server)
}

/// Accepts one request and never replies, so the client observes a request or
/// idle timeout. The request line is reported once the head has been read and
/// [`SilentUpstream::abort`] closes the connection.
pub(crate) struct SilentUpstream {
    pub(crate) address: SocketAddr,
    pub(crate) request_line: oneshot::Receiver<String>,
    server: JoinHandle<()>,
}

impl SilentUpstream {
    /// Stops the mock upstream and drops its socket.
    pub(crate) fn abort(&self) {
        self.server.abort();
    }
}

/// Starts a [`SilentUpstream`] on an ephemeral loopback port.
pub(crate) async fn spawn_silent_upstream() -> SilentUpstream {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("silent mock upstream listener");
    let address = listener.local_addr().expect("silent mock upstream address");
    let (request_line_sender, request_line) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("silent mock request");
        let mut stream = BufReader::new(stream);
        let request = read_request_head(&mut stream).await;
        let _ = request_line_sender.send(request.request_line);
        std::future::pending::<()>().await;
    });
    SilentUpstream {
        address,
        request_line,
        server,
    }
}

async fn read_request_head(stream: &mut BufReader<TcpStream>) -> MockRequest {
    let mut request_line = String::new();
    stream
        .read_line(&mut request_line)
        .await
        .expect("mock request line");
    let mut authorization = None;
    loop {
        let mut line = String::new();
        stream
            .read_line(&mut line)
            .await
            .expect("mock request header");
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.trim_end().split_once(':')
            && name.eq_ignore_ascii_case("authorization")
        {
            authorization = Some(value.trim().to_owned());
        }
    }
    MockRequest {
        request_line: request_line.trim_end().to_owned(),
        authorization,
    }
}

async fn write_response(stream: &mut BufReader<TcpStream>, response: &MockResponse) {
    let reason = if (200..300).contains(&response.status) {
        "OK"
    } else {
        "Unavailable"
    };
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    );
    stream
        .get_mut()
        .write_all(head.as_bytes())
        .await
        .expect("mock response headers");
    if let Some(delay) = response.delay_before_body {
        tokio::time::sleep(delay).await;
    }
    stream
        .get_mut()
        .write_all(&response.body)
        .await
        .expect("mock response body");
    if let Some(hold) = response.hold_after_body {
        tokio::time::sleep(hold).await;
    }
}
