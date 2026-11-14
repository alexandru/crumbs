use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Extension, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use crumbs_database::identity::UserSession;
use crumbs_database::profile_logs::{
    CreateLog, LogFilter, PatchLog, ProfileLogError, ProfileLogId, ProfileLogs,
};
use crumbs_database::profiles::ProfileId;
use serde_json::json;

use crate::ErrorResponse;

pub(super) async fn list<S: ProfileLogs + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    id: Result<Path<ProfileId>, PathRejection>,
    query: Result<Query<LogFilter>, QueryRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Path(profile_id) = id.map_err(invalid_request)?;
    let Query(filter) = query.map_err(invalid_request)?;

    let page = store
        .list_profile_logs(session.user_id, profile_id, &filter)
        .await
        .map_err(profile_log_error)?
        .ok_or_else(not_found)?;
    Ok(Json(page))
}

pub(super) async fn create<S: ProfileLogs + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    id: Result<Path<ProfileId>, PathRejection>,
    input: Result<Json<CreateLog>, JsonRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Path(profile_id) = id.map_err(invalid_request)?;
    let Json(input) = input.map_err(invalid_request)?;

    let log = store
        .create_profile_log(session.user_id, profile_id, &input)
        .await
        .map_err(profile_log_error)?
        .ok_or_else(not_found)?;
    Ok((
        StatusCode::CREATED,
        [(
            header::LOCATION,
            format!("/profiles/{}/logs/{}", profile_id.get(), log.id.get()),
        )],
        Json(log),
    ))
}

pub(super) async fn get<S: ProfileLogs + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    path: Result<Path<(ProfileId, ProfileLogId)>, PathRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Path((profile_id, log_id)) = path.map_err(invalid_request)?;

    let log = store
        .profile_log(session.user_id, profile_id, log_id)
        .await
        .map_err(profile_log_error)?
        .ok_or_else(not_found)?;
    Ok(Json(log))
}

pub(super) async fn patch<S: ProfileLogs + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    path: Result<Path<(ProfileId, ProfileLogId)>, PathRejection>,
    input: Result<Json<PatchLog>, JsonRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Path((profile_id, log_id)) = path.map_err(invalid_request)?;
    let Json(patch) = input.map_err(invalid_request)?;

    let log = store
        .patch_profile_log(session.user_id, profile_id, log_id, &patch)
        .await
        .map_err(profile_log_error)?
        .ok_or_else(not_found)?;
    Ok(Json(log))
}

pub(super) async fn delete<S: ProfileLogs + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    path: Result<Path<(ProfileId, ProfileLogId)>, PathRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Path((profile_id, log_id)) = path.map_err(invalid_request)?;

    if store
        .delete_profile_log(session.user_id, profile_id, log_id)
        .await
        .map_err(profile_log_error)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found())
    }
}

fn profile_log_error(error: ProfileLogError) -> ErrorResponse {
    let code = error.code();
    let (status, message) = match error {
        ProfileLogError::InvalidInput(message) => (StatusCode::BAD_REQUEST, message),
        ProfileLogError::InvalidStoredData
        | ProfileLogError::Storage(_)
        | ProfileLogError::Clock(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "profile log operation failed".to_owned(),
        ),
    };
    error_response(status, code, message)
}

fn not_found() -> ErrorResponse {
    error_response(StatusCode::NOT_FOUND, "not_found", "profile log not found")
}

fn invalid_request(error: impl std::fmt::Display) -> ErrorResponse {
    error_response(StatusCode::BAD_REQUEST, "invalid_input", error.to_string())
}

fn error_response(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> ErrorResponse {
    (status, Json(json!({"code": code, "error": message.into()})))
}

#[cfg(test)]
#[path = "../../unit-tests/profile_logs/handlers.rs"]
mod tests;
