use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Extension, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use crumbs_database::identity::UserSession;
use crumbs_database::profile_logs::{
    CreateLog, LogFilter, PatchLog, ProfileLogError, ProfileLogId, ProfileLogs,
};
use crumbs_database::profiles::ProfileId;
use serde_json::json;

pub(super) async fn list<S: ProfileLogs + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    id: Result<Path<ProfileId>, PathRejection>,
    query: Result<Query<LogFilter>, QueryRejection>,
) -> Response {
    let Path(profile_id) = match id {
        Ok(id) => id,
        Err(error) => return invalid_request(error.to_string()),
    };
    let Query(filter) = match query {
        Ok(query) => query,
        Err(error) => return invalid_request(error.to_string()),
    };
    match store
        .list_profile_logs(session.user_id, profile_id, &filter)
        .await
    {
        Ok(Some(page)) => Json(page).into_response(),
        Ok(None) => not_found(),
        Err(error) => profile_log_error(error),
    }
}

pub(super) async fn create<S: ProfileLogs + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    id: Result<Path<ProfileId>, PathRejection>,
    input: Result<Json<CreateLog>, JsonRejection>,
) -> Response {
    let Path(profile_id) = match id {
        Ok(id) => id,
        Err(error) => return invalid_request(error.to_string()),
    };
    let Json(input) = match input {
        Ok(input) => input,
        Err(error) => return invalid_request(error.to_string()),
    };
    match store
        .create_profile_log(session.user_id, profile_id, &input)
        .await
    {
        Ok(Some(log)) => (
            StatusCode::CREATED,
            [(
                header::LOCATION,
                format!("/profiles/{}/logs/{}", profile_id.get(), log.id.get()),
            )],
            Json(log),
        )
            .into_response(),
        Ok(None) => not_found(),
        Err(error) => profile_log_error(error),
    }
}

pub(super) async fn get<S: ProfileLogs + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    path: Result<Path<(ProfileId, ProfileLogId)>, PathRejection>,
) -> Response {
    let Path((profile_id, log_id)) = match path {
        Ok(path) => path,
        Err(error) => return invalid_request(error.to_string()),
    };
    match store.profile_log(session.user_id, profile_id, log_id).await {
        Ok(Some(log)) => Json(log).into_response(),
        Ok(None) => not_found(),
        Err(error) => profile_log_error(error),
    }
}

pub(super) async fn patch<S: ProfileLogs + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    path: Result<Path<(ProfileId, ProfileLogId)>, PathRejection>,
    input: Result<Json<PatchLog>, JsonRejection>,
) -> Response {
    let Path((profile_id, log_id)) = match path {
        Ok(path) => path,
        Err(error) => return invalid_request(error.to_string()),
    };
    let Json(patch) = match input {
        Ok(input) => input,
        Err(error) => return invalid_request(error.to_string()),
    };
    match store
        .patch_profile_log(session.user_id, profile_id, log_id, &patch)
        .await
    {
        Ok(Some(log)) => Json(log).into_response(),
        Ok(None) => not_found(),
        Err(error) => profile_log_error(error),
    }
}

pub(super) async fn delete<S: ProfileLogs + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    path: Result<Path<(ProfileId, ProfileLogId)>, PathRejection>,
) -> Response {
    let Path((profile_id, log_id)) = match path {
        Ok(path) => path,
        Err(error) => return invalid_request(error.to_string()),
    };
    match store
        .delete_profile_log(session.user_id, profile_id, log_id)
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(),
        Err(error) => profile_log_error(error),
    }
}

fn profile_log_error(error: ProfileLogError) -> Response {
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

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "profile log not found")
}

fn invalid_request(message: String) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_input", message)
}

fn error_response(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    (status, Json(json!({"code": code, "error": message.into()}))).into_response()
}

#[cfg(test)]
#[path = "../../unit-tests/profile_logs/handlers.rs"]
mod tests;
