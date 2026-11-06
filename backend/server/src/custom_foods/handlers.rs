//! REST management of foods created in Crumbs.

use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use crumbs_database::foods::{CustomFoodInput, CustomFoodStore};
use serde_json::json;

use super::model::{FoodIdParameter, ListRequest, ListResponse};

pub(super) async fn list<S: CustomFoodStore + Send + 'static>(
    State(store): State<Arc<S>>,
    request: Result<Query<ListRequest>, QueryRejection>,
) -> Response {
    let Query(request) = match request {
        Ok(request) => request,
        Err(error) => return failure(StatusCode::BAD_REQUEST, error.to_string()),
    };
    match store.list_custom_foods(request.limit).await {
        Ok(foods) => Json(ListResponse { foods }).into_response(),
        Err(_) => storage_failure(),
    }
}

pub(super) async fn create<S: CustomFoodStore + Send + 'static>(
    State(store): State<Arc<S>>,
    input: Result<Json<CustomFoodInput>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(error) => return failure(StatusCode::BAD_REQUEST, error.to_string()),
    };
    match store.create_custom_food(&input).await {
        Ok(food) => (
            StatusCode::CREATED,
            [(header::LOCATION, format!("/foods/custom/{}", food.id.0))],
            Json(food),
        )
            .into_response(),
        Err(_) => storage_failure(),
    }
}

pub(super) async fn get<S: CustomFoodStore + Send + 'static>(
    State(store): State<Arc<S>>,
    id: Result<Path<FoodIdParameter>, PathRejection>,
) -> Response {
    let Path(id) = match id {
        Ok(id) => id,
        Err(error) => return failure(StatusCode::BAD_REQUEST, error.to_string()),
    };
    match store.custom_food(id.food_id()).await {
        Ok(Some(food)) => Json(food).into_response(),
        Ok(None) => not_found(),
        Err(_) => storage_failure(),
    }
}

pub(super) async fn update<S: CustomFoodStore + Send + 'static>(
    State(store): State<Arc<S>>,
    id: Result<Path<FoodIdParameter>, PathRejection>,
    input: Result<Json<CustomFoodInput>, JsonRejection>,
) -> Response {
    let Path(id) = match id {
        Ok(id) => id,
        Err(error) => return failure(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let Json(input) = match input {
        Ok(input) => input,
        Err(error) => return failure(StatusCode::BAD_REQUEST, error.to_string()),
    };
    match store.update_custom_food(id.food_id(), &input).await {
        Ok(Some(food)) => Json(food).into_response(),
        Ok(None) => not_found(),
        Err(_) => storage_failure(),
    }
}

pub(super) async fn delete<S: CustomFoodStore + Send + 'static>(
    State(store): State<Arc<S>>,
    id: Result<Path<FoodIdParameter>, PathRejection>,
) -> Response {
    let Path(id) = match id {
        Ok(id) => id,
        Err(error) => return failure(StatusCode::BAD_REQUEST, error.to_string()),
    };
    match store.delete_custom_food(id.food_id()).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(),
        Err(_) => storage_failure(),
    }
}

fn not_found() -> Response {
    failure(StatusCode::NOT_FOUND, "custom food not found")
}

fn storage_failure() -> Response {
    failure(
        StatusCode::INTERNAL_SERVER_ERROR,
        "custom food operation failed",
    )
}

fn failure(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error": message.into()}))).into_response()
}
