//! Request and tool lifecycle events.

use std::time::Instant;

use axum::body::Body;
use axum::extract::MatchedPath;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use tracing::{Instrument, Span};

pub(crate) enum ToolOutcome {
    Success,
    Error,
    UnknownTool,
    InsufficientScope,
    InputRequired,
    TaskAccepted,
    ProtocolError,
    Other,
}

impl ToolOutcome {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error => "error",
            Self::UnknownTool => "unknown_tool",
            Self::InsufficientScope => "insufficient_scope",
            Self::InputRequired => "input_required",
            Self::TaskAccepted => "task_accepted",
            Self::ProtocolError => "protocol_error",
            Self::Other => "other",
        }
    }

    const fn is_error(&self) -> bool {
        matches!(
            self,
            Self::Error | Self::UnknownTool | Self::InsufficientScope | Self::ProtocolError
        )
    }
}

pub(crate) struct Lifecycle {
    span: Span,
    started: Instant,
    finished: bool,
}

impl Lifecycle {
    pub(crate) fn start(span: Span) -> Self {
        span.in_scope(|| tracing::info!(event = "start"));
        Self {
            span,
            started: Instant::now(),
            finished: false,
        }
    }

    pub(crate) fn finish(mut self, outcome: ToolOutcome, tool: &str) {
        self.finished = true;
        let elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        self.span.in_scope(|| {
            if outcome.is_error() {
                tracing::warn!(
                    event = "result",
                    tool,
                    outcome = outcome.as_str(),
                    elapsed_ms
                );
            } else {
                tracing::info!(event = "result", outcome = outcome.as_str(), elapsed_ms);
            }
        });
    }

    fn finish_http(mut self, status: StatusCode, method: &Method, route: &str) {
        self.finished = true;
        let response_headers_elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        self.span.in_scope(|| {
            if status.is_server_error() {
                tracing::warn!(
                    event = "response_headers",
                    %method,
                    route,
                    status = status.as_u16(),
                    response_headers_elapsed_ms
                );
            } else {
                tracing::info!(
                    event = "response_headers",
                    status = status.as_u16(),
                    response_headers_elapsed_ms
                );
            }
        });
    }
}

impl Drop for Lifecycle {
    fn drop(&mut self) {
        if !self.finished {
            let elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.0;
            self.span
                .in_scope(|| tracing::info!(event = "cancelled", elapsed_ms));
        }
    }
}

pub(crate) async fn http(request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let matched_path = request.extensions().get::<MatchedPath>().cloned();
    let route = matched_path
        .as_ref()
        .map_or("<unmatched>", MatchedPath::as_str);
    let span = tracing::info_span!("http_request", %method, route);
    let instrument_span = span.clone();
    async move {
        let lifecycle = Lifecycle::start(span);
        let response = next.run(request).await;
        lifecycle.finish_http(response.status(), &method, route);
        response
    }
    .instrument(instrument_span)
    .await
}

#[cfg(test)]
#[path = "../unit-tests/observability.rs"]
mod tests;
