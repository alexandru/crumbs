use axum::body::to_bytes;
use axum::http::{StatusCode, header::CONTENT_TYPE};

use super::*;

#[tokio::test]
async fn stylesheet_is_served_as_css() -> Result<(), axum::Error> {
    let response = stylesheet();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/css; charset=utf-8")
    );

    let body = to_bytes(response.into_body(), usize::MAX).await?;
    assert!(String::from_utf8_lossy(&body).contains(".login-card"));
    Ok(())
}
