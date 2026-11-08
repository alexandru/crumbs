use std::error::Error;

use serde_json::{Value, json};

use crate::profile_logs::tests::profile_log_error_cases;

use super::profile_log_error;

#[tokio::test]
async fn rest_profile_log_error_adapter_preserves_status_json_and_redacts_internal_details()
-> Result<(), Box<dyn Error>> {
    for case in profile_log_error_cases() {
        let response = profile_log_error(case.error);

        assert_eq!(response.status(), case.status, "{}", case.name);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        let body: Value = serde_json::from_slice(&body)?;
        assert_eq!(
            body,
            json!({"code": case.code, "error": case.message}),
            "{}",
            case.name
        );
    }
    Ok(())
}
