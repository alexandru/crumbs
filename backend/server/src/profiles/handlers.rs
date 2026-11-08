use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Extension, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use crumbs_database::identity::UserSession;
use crumbs_database::profiles::{
    CreateProfile, PatchProfile, ProfileError, ProfileId, ProfileListQuery, Profiles,
};
use serde_json::json;

pub(super) async fn list<S: Profiles + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    request: Result<Query<ProfileListQuery>, QueryRejection>,
) -> Response {
    let Query(query) = match request {
        Ok(request) => request,
        Err(error) => return invalid_request(error.to_string()),
    };
    match store.list_profiles(session.user_id, &query).await {
        Ok(profiles) => Json(profiles).into_response(),
        Err(error) => profile_error(error),
    }
}

pub(super) async fn create<S: Profiles + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    input: Result<Json<CreateProfile>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(error) => return invalid_request(error.to_string()),
    };
    match store.create_profile(session.user_id, &input).await {
        Ok(profile) => (
            StatusCode::CREATED,
            [(header::LOCATION, format!("/profiles/{}", profile.id.get()))],
            Json(profile),
        )
            .into_response(),
        Err(error) => profile_error(error),
    }
}

pub(super) async fn get<S: Profiles + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    id: Result<Path<ProfileId>, PathRejection>,
) -> Response {
    let Path(id) = match id {
        Ok(id) => id,
        Err(error) => return invalid_request(error.to_string()),
    };
    match store.profile(session.user_id, id).await {
        Ok(Some(profile)) => Json(profile).into_response(),
        Ok(None) => not_found(),
        Err(error) => profile_error(error),
    }
}

pub(super) async fn patch<S: Profiles + Send + 'static>(
    State(store): State<Arc<S>>,
    Extension(session): Extension<UserSession>,
    id: Result<Path<ProfileId>, PathRejection>,
    input: Result<Json<PatchProfile>, JsonRejection>,
) -> Response {
    let Path(id) = match id {
        Ok(id) => id,
        Err(error) => return invalid_request(error.to_string()),
    };
    let Json(patch) = match input {
        Ok(input) => input,
        Err(error) => return invalid_request(error.to_string()),
    };
    match store.patch_profile(session.user_id, id, &patch).await {
        Ok(Some(profile)) => Json(profile).into_response(),
        Ok(None) => not_found(),
        Err(error) => profile_error(error),
    }
}

fn profile_error(error: ProfileError) -> Response {
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

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "profile not found")
}

fn invalid_request(message: String) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_input", message)
}

fn error_response(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    (status, Json(json!({"code": code, "error": message.into()}))).into_response()
}

#[cfg(test)]
#[path = "../../unit-tests/profiles/handlers.rs"]
mod tests;
