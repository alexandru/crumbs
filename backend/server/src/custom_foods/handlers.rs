//! REST management of foods created in Crumbs.

use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use crumbs_database::foods::{CustomFoodInput, CustomFoodStore};
use serde_json::json;

use crate::ErrorResponse;

use super::model::{FoodIdParameter, ListRequest, ListResponse};

pub(super) async fn list<S: CustomFoodStore + Send + 'static>(
    State(store): State<Arc<S>>,
    request: Result<Query<ListRequest>, QueryRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Query(request) =
        request.map_err(|error| failure(StatusCode::BAD_REQUEST, error.to_string()))?;

    let foods = store
        .list_custom_foods(request.limit)
        .await
        .map_err(|_| storage_failure())?;
    Ok(Json(ListResponse { foods }))
}

pub(super) async fn create<S: CustomFoodStore + Send + 'static>(
    State(store): State<Arc<S>>,
    input: Result<Json<CustomFoodInput>, JsonRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Json(input) = input.map_err(|error| failure(StatusCode::BAD_REQUEST, error.to_string()))?;

    let food = store
        .create_custom_food(&input)
        .await
        .map_err(|_| storage_failure())?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/foods/custom/{}", food.id.0))],
        Json(food),
    ))
}

pub(super) async fn get<S: CustomFoodStore + Send + 'static>(
    State(store): State<Arc<S>>,
    id: Result<Path<FoodIdParameter>, PathRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Path(id) = id.map_err(|error| failure(StatusCode::BAD_REQUEST, error.to_string()))?;

    let food = store
        .custom_food(id.food_id())
        .await
        .map_err(|_| storage_failure())?
        .ok_or_else(not_found)?;
    Ok(Json(food))
}

pub(super) async fn update<S: CustomFoodStore + Send + 'static>(
    State(store): State<Arc<S>>,
    id: Result<Path<FoodIdParameter>, PathRejection>,
    input: Result<Json<CustomFoodInput>, JsonRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Path(id) = id.map_err(|error| failure(StatusCode::BAD_REQUEST, error.to_string()))?;
    let Json(input) = input.map_err(|error| failure(StatusCode::BAD_REQUEST, error.to_string()))?;

    let food = store
        .update_custom_food(id.food_id(), &input)
        .await
        .map_err(|_| storage_failure())?
        .ok_or_else(not_found)?;
    Ok(Json(food))
}

pub(super) async fn delete<S: CustomFoodStore + Send + 'static>(
    State(store): State<Arc<S>>,
    id: Result<Path<FoodIdParameter>, PathRejection>,
) -> Result<impl IntoResponse, ErrorResponse> {
    let Path(id) = id.map_err(|error| failure(StatusCode::BAD_REQUEST, error.to_string()))?;

    if store
        .delete_custom_food(id.food_id())
        .await
        .map_err(|_| storage_failure())?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found())
    }
}

fn not_found() -> ErrorResponse {
    failure(StatusCode::NOT_FOUND, "custom food not found")
}

fn storage_failure() -> ErrorResponse {
    failure(
        StatusCode::INTERNAL_SERVER_ERROR,
        "custom food operation failed",
    )
}

fn failure(status: StatusCode, message: impl Into<String>) -> ErrorResponse {
    (status, Json(json!({"error": message.into()})))
}
