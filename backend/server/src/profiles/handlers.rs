use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Extension, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use crumbs_database::identity::UserSession;
use crumbs_database::profiles::{
    CreateProfile, PatchProfile, ProfileError, ProfileId, ProfileListQuery, Profiles,
};
use serde_json::json;

use crate::ErrorResponse;

pub(super) async fn list<S: Profiles + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    request: Result<Query<ProfileListQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Query(query) = request.map_err(invalid_request)?;

    let profiles = store
        .list_profiles(session.user_id, &query)
        .await
        .map_err(profile_error)?;
    Ok(Json(profiles))
}

pub(super) async fn create<S: Profiles + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    input: Result<Json<CreateProfile>, JsonRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Json(input) = input.map_err(invalid_request)?;

    let profile = store
        .create_profile(session.user_id, &input)
        .await
        .map_err(profile_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/profiles/{}", profile.id.get()))],
        Json(profile),
    ))
}

pub(super) async fn get<S: Profiles + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    id: Result<Path<ProfileId>, PathRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Path(id) = id.map_err(invalid_request)?;

    let profile = store
        .profile(session.user_id, id)
        .await
        .map_err(profile_error)?
        .ok_or_else(not_found)?;
    Ok(Json(profile))
}

pub(super) async fn patch<S: Profiles + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    id: Result<Path<ProfileId>, PathRejection>,
    input: Result<Json<PatchProfile>, JsonRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Path(id) = id.map_err(invalid_request)?;
    let Json(patch) = input.map_err(invalid_request)?;

    let profile = store
        .patch_profile(session.user_id, id, &patch)
        .await
        .map_err(profile_error)?
        .ok_or_else(not_found)?;
    Ok(Json(profile))
}

fn profile_error(error: ProfileError) -> ErrorResponse {
    let code = error.code();
    let (status, message) = match error {
        ProfileError::InvalidInput(message) | ProfileError::Calculation(message) => {
            (StatusCode::BAD_REQUEST, message)
        }
        ProfileError::DuplicateName => (
            StatusCode::CONFLICT,
            "profile name already exists for this account".to_owned(),
        ),
        ProfileError::InvalidStoredData | ProfileError::Storage(_) | ProfileError::Clock(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "profile operation failed".to_owned(),
        ),
    };

    error_response(status, code, message)
}

fn not_found() -> ErrorResponse {
    error_response(StatusCode::NOT_FOUND, "not_found", "profile not found")
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
#[path = "../../unit-tests/profiles/handlers.rs"]
mod tests;
