//! REST management of foods created in Crumbs.

use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use crumbs_database::{CustomFoodInput, CustomFoodStore, FoodId, FoodSearchResult, SearchLimit};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListRequest {
    #[serde(default)]
    pub(crate) limit: SearchLimit,
}

#[derive(Debug, Serialize)]
pub(crate) struct ListResponse {
    pub(crate) foods: Vec<FoodSearchResult>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(try_from = "i64")]
pub(crate) struct FoodIdParameter(FoodId);

impl FoodIdParameter {
    pub(crate) const fn food_id(self) -> FoodId {
        self.0
    }
}

impl TryFrom<i64> for FoodIdParameter {
    type Error = &'static str;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value <= 0 {
            return Err("food ID must be positive");
        }
        Ok(Self(FoodId(value)))
    }
}

pub(crate) async fn list<S: CustomFoodStore + Send + 'static>(
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

pub(crate) async fn create<S: CustomFoodStore + Send + 'static>(
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

pub(crate) async fn get<S: CustomFoodStore + Send + 'static>(
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

pub(crate) async fn update<S: CustomFoodStore + Send + 'static>(
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

pub(crate) async fn delete<S: CustomFoodStore + Send + 'static>(
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
