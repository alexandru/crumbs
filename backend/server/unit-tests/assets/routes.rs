use std::error::Error;

use axum::body::{Body, to_bytes};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::*;

#[tokio::test]
async fn canonical_and_compatibility_paths_serve_the_same_css_and_headers()
-> Result<(), Box<dyn Error>> {
    let app = routes();
    let canonical = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/main.css")
                .body(Body::empty())?,
        )
        .await?;
    let compatibility = app
        .oneshot(
            Request::builder()
                .uri("/assets/login.css")
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(canonical.status(), StatusCode::OK);
    assert_eq!(compatibility.status(), StatusCode::OK);
    for response in [&canonical, &compatibility] {
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/css; charset=utf-8")
        );
        assert_eq!(
            response
                .headers()
                .get(CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-cache")
        );
    }

    let canonical_css = to_bytes(canonical.into_body(), usize::MAX).await?;
    let compatibility_css = to_bytes(compatibility.into_body(), usize::MAX).await?;
    assert_eq!(canonical_css, compatibility_css);
    assert!(String::from_utf8_lossy(&canonical_css).contains(".login-card"));
    Ok(())
}
