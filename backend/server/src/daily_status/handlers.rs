use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use crumbs_database::daily_status::{DailyStatusError, DailyStatusQuery, DailyStatusStore};
use crumbs_database::identity::UserSession;
use crumbs_database::profiles::{ProfileError, ProfileId};
use serde_json::json;

use crate::ErrorResponse;

pub(super) async fn get<S: DailyStatusStore + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    id: Result<Path<ProfileId>, PathRejection>,
    query: Result<Query<DailyStatusQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Path(profile_id) = id.map_err(invalid_request)?;
    let Query(query) = query.map_err(invalid_request)?;
    let report = store
        .daily_status(session.user_id, profile_id, &query)
        .await
        .map_err(status_error)?
        .ok_or_else(not_found)?;
    Ok(Json(report))
}

pub(super) fn status_error(error: DailyStatusError) -> ErrorResponse {
    let code = error.code();
    let (status, message) = match error {
        DailyStatusError::InvalidInput(message)
        | DailyStatusError::Profile(ProfileError::InvalidInput(message))
        | DailyStatusError::Profile(ProfileError::Calculation(message)) => {
            (StatusCode::BAD_REQUEST, message)
        }
        DailyStatusError::Profile(ProfileError::DuplicateName) => (
            StatusCode::CONFLICT,
            "profile name already exists for this account".to_owned(),
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "daily status operation failed".to_owned(),
        ),
    };
    (status, Json(json!({"code": code, "error": message})))
}

pub(super) fn not_found() -> ErrorResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"code": "not_found", "error": "profile not found"})),
    )
}

fn invalid_request(error: impl std::fmt::Display) -> ErrorResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"code": "invalid_input", "error": error.to_string()})),
    )
}
