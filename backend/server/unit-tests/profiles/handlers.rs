use std::error::Error;

use axum::response::IntoResponse;

use serde_json::{Value, json};

#[tokio::test]
async fn profile_error_responses_preserve_status_json_and_redact_sources()
-> Result<(), Box<dyn Error>> {
    for case in crate::profiles::tests::profile_error_cases() {
        let response = super::profile_error(case.error).into_response();
        assert_eq!(response.status(), case.status);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        let body = std::str::from_utf8(&body)?;
        assert!(!body.contains("private storage source detail"));
        assert!(!body.contains("private clock source detail"));
        let payload: Value = serde_json::from_str(body)?;
        assert_eq!(payload, json!({"code": case.code, "error": case.message}));
    }

    Ok(())
}
