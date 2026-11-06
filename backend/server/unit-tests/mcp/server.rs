use std::error::Error;

use axum::http::StatusCode;
use serde_json::json;
use tower::ServiceExt;

use crate::tests::{TestApplication, mcp_request_with_origin};

#[tokio::test]
async fn hosted_host_is_accepted_and_unapproved_browser_origin_is_rejected()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    let hosted = app.request(mcp_request_with_origin(json!({
        "jsonrpc":"2.0", "id":1, "method":"initialize",
        "params": {"protocolVersion":"2025-11-25", "capabilities":{}, "clientInfo":{"name":"hosted","version":"1"}}
    }), None)?).await?;
    assert_eq!(hosted.status, StatusCode::OK, "{:?}", hosted.json);
    let rejected = app
        .router
        .clone()
        .oneshot(mcp_request_with_origin(
            json!({"jsonrpc":"2.0","id":2,"method":"ping"}),
            Some("https://evil.example"),
        )?)
        .await?;
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    Ok(())
}
