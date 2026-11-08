use std::error::Error;

use serde_json::{Value, json};

use crate::profiles::tests::profile_error_cases;

use super::profile_error;

#[tokio::test]
async fn profile_error_responses_preserve_status_json_and_redact_sources()
-> Result<(), Box<dyn Error>> {
    for case in profile_error_cases() {
        let response = profile_error(case.error);
        assert_eq!(response.status(), case.status);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        let body = std::str::from_utf8(&body)?;
        assert!(!body.contains("private storage source detail"));
        assert!(!body.contains("private clock source detail"));
        let payload: Value = serde_json::from_str(&body)?;
        assert_eq!(payload, json!({"code": case.code, "error": case.message}));
    }

    Ok(())
}
