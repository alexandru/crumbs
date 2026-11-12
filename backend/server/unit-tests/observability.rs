use std::error::Error;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use serde_json::json;
use tower::ServiceExt;
use tracing_subscriber::{filter::LevelFilter, layer::SubscriberExt, reload};

use crate::oauth::tests::TEST_MCP_ACCESS_TOKEN;
use crate::tests::{TestApplication, http_request, mcp_call_router_with_token, mcp_initialize};

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Write for Buffer {
    fn write(&mut self, value: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("log buffer poisoned"))?
            .extend_from_slice(value);
        Ok(value.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn lifecycle_logs_route_status_tool_outcome_and_cancellation_without_request_content()
-> Result<(), Box<dyn Error>> {
    let buffer = Buffer::default();
    let output = buffer.clone();
    let (filter, filter_handle) = reload::Layer::new(LevelFilter::INFO);
    let formatter = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .without_time()
        .with_writer(move || buffer.clone());
    let subscriber = tracing_subscriber::registry().with(filter).with(formatter);
    // Parallel requests must use the same subscriber when they register log callsites.
    tracing::subscriber::set_global_default(subscriber)?;
    let info_logs = async {
        let app = TestApplication::new().await?;
        let response = app
            .request(http_request("/foods/search?query=private_query_marker")?)
            .await?;
        assert!(response.status.is_success());
        let denied = app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/foods/search?query=private_denied_marker")
                    .header("authorization", "Bearer private_token_marker")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(denied.status(), axum::http::StatusCode::UNAUTHORIZED);
        mcp_initialize(&app).await?;
        let success = mcp_call_router_with_token(
            &app.router,
            20,
            "search_foods",
            json!({"query":"private_argument_marker"}),
            TEST_MCP_ACCESS_TOKEN,
        )
        .await?;
        assert_eq!(success.status, axum::http::StatusCode::OK);
        let failure = mcp_call_router_with_token(
            &app.router,
            21,
            "search_foods",
            json!({"query":"("}),
            TEST_MCP_ACCESS_TOKEN,
        )
        .await?;
        assert_eq!(failure.status, axum::http::StatusCode::OK);
        assert_eq!(failure.json["result"]["isError"], true);
        let denied = mcp_call_router_with_token(
            &app.router,
            22,
            "create_custom_food",
            json!({"food":{"description":"private_body_marker"}}),
            TEST_MCP_ACCESS_TOKEN,
        )
        .await?;
        assert_eq!(denied.json["result"]["isError"], true);
        let _ = mcp_call_router_with_token(
            &app.router,
            23,
            "private_unknown_marker",
            json!({}),
            TEST_MCP_ACCESS_TOKEN,
        )
        .await?;
        let slow = Router::new()
            .route(
                "/slow",
                get(|| async { std::future::pending::<()>().await }),
            )
            .layer(axum::middleware::from_fn(super::http));
        let request = Request::builder().uri("/slow").body(Body::empty())?;
        let _ =
            tokio::time::timeout(std::time::Duration::from_millis(10), slow.oneshot(request)).await;
        filter_handle.reload(LevelFilter::WARN)?;
        let info_logs = {
            let mut guard = output
                .0
                .lock()
                .map_err(|_| io::Error::other("log buffer poisoned"))?;
            let logs = String::from_utf8(guard.clone())?;
            guard.clear();
            logs
        };
        let failed_route = Router::new()
            .route(
                "/warn-test/{item}",
                get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
            )
            .layer(axum::middleware::from_fn(super::http));
        let response = failed_route
            .oneshot(
                Request::builder()
                    .uri("/warn-test/private_path_marker")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let failure = mcp_call_router_with_token(
            &app.router,
            24,
            "search_foods",
            json!({"query":"("}),
            TEST_MCP_ACCESS_TOKEN,
        )
        .await?;
        assert_eq!(failure.json["result"]["isError"], true);
        Ok::<String, Box<dyn Error>>(info_logs)
    }
    .await?;
    let logs = String::from_utf8(
        output
            .0
            .lock()
            .map_err(|_| io::Error::other("log buffer poisoned"))?
            .clone(),
    )?;
    assert!(info_logs.contains("event=\"start\""), "{info_logs}");
    assert!(
        info_logs.contains("response_headers_elapsed_ms"),
        "{info_logs}"
    );
    assert!(info_logs.contains("status=401"), "{info_logs}");
    assert!(info_logs.contains("/foods/search"), "{info_logs}");
    assert!(info_logs.contains("search_foods"), "{info_logs}");
    assert!(info_logs.contains("outcome=\"success\""), "{info_logs}");
    assert!(info_logs.contains("outcome=\"error\""), "{info_logs}");
    assert!(
        info_logs.contains("outcome=\"unknown_tool\""),
        "{info_logs}"
    );
    assert!(info_logs.contains("insufficient_scope"), "{info_logs}");
    assert!(info_logs.contains("event=\"cancelled\""), "{info_logs}");
    let http_warning = logs
        .lines()
        .find(|line| line.contains("route=\"/warn-test/{item}\""))
        .ok_or("missing HTTP warning")?;
    assert!(
        http_warning.contains("WARN")
            && http_warning.contains("method=GET")
            && http_warning.contains("status=500"),
        "{http_warning}"
    );
    let tool_warning = logs
        .lines()
        .find(|line| line.contains("tool=\"search_foods\"") && line.contains("outcome=\"error\""))
        .ok_or("missing tool warning")?;
    assert!(tool_warning.contains("WARN"), "{tool_warning}");
    assert!(!logs.contains("event=\"start\""), "{logs}");
    for marker in [
        "private_query_marker",
        "private_denied_marker",
        "private_argument_marker",
        "private_body_marker",
        "private_unknown_marker",
        "private_token_marker",
        "private_path_marker",
    ] {
        assert!(!info_logs.contains(marker));
        assert!(!logs.contains(marker));
    }
    Ok(())
}
