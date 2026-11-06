use axum::body::to_bytes;
use axum::http::{StatusCode, header::CONTENT_TYPE};

use super::*;

#[test]
fn login_escapes_untrusted_client_and_error_text() {
    let markup = login(LoginPage {
        request_id: "request-<id>",
        csrf_token: "token-\"quoted\"",
        client_name: "<img src=x onerror=alert(1)>",
        redirect_origin: "https://example.test/?a=<b>",
        scopes: FoodScopes::Read,
        error: Some("<script>alert(1)</script>"),
    })
    .into_string();

    assert!(markup.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(markup.contains("https://example.test/?a=&lt;b&gt;"));
    assert!(markup.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!markup.contains("<script>alert(1)</script>"));
    assert!(markup.contains("name=\"request_id\" value=\"request-&lt;id&gt;\""));
    assert!(markup.contains("name=\"csrf_token\" value=\"token-&quot;quoted&quot;\""));
    assert!(markup.contains("name=\"username\""));
    assert!(markup.contains("name=\"password\""));
    assert!(markup.contains("name=\"decision\" value=\"approve\""));
    assert!(markup.contains("name=\"decision\" value=\"deny\" formnovalidate"));
    assert!(markup.contains("role=\"alert\""));
    assert!(markup.contains("Read the food library"));
    assert!(markup.contains("view nutrition data"));
    assert!(markup.contains("Only continue if you started this connection."));
}

#[test]
fn login_shows_only_the_requested_permissions() {
    let markup = login(LoginPage {
        request_id: "request-id",
        csrf_token: "csrf-token",
        client_name: "Food manager",
        redirect_origin: "https://example.test",
        scopes: FoodScopes::Write,
        error: None,
    })
    .into_string();

    assert!(markup.contains("Create, edit, and delete custom foods"));
    assert!(!markup.contains("Read the food library"));
    assert!(!markup.contains("view nutrition data"));
}

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
