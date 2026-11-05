//! HTTP handlers for the greeting and food search routes.

use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use crumbs_database::{FoodReader, SearchError};
use maud::{DOCTYPE, Markup, html};
use serde::Serialize;

use crate::search::{self, SearchRequest};

pub(super) async fn hello() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Crumbs" }
                link rel="stylesheet" href="/assets/login.css";
            }
            body {
                main class="page-shell" {
                    section class="login-card" aria-labelledby="page-title" {
                        header class="brand" aria-label="Crumbs" {
                            span class="brand-mark" aria-hidden="true" {
                                span class="crumb crumb-one" {}
                                span class="crumb crumb-two" {}
                                span class="crumb crumb-three" {}
                            }
                            span class="brand-name" { "crumbs" }
                        }

                        p class="eyebrow" { "Food library" }
                        h1 id="page-title" { "Hello, welcome to Crumbs." }
                        p class="intro" { "A little clarity about what you eat." }
                        p class="safety-note" { "Your food library is ready." }
                    }
                }
            }
        }
    }
}

pub(super) async fn search_foods<S: FoodReader + Send + 'static>(
    State(reader): State<Arc<S>>,
    request: Result<Query<SearchRequest>, QueryRejection>,
) -> Response {
    let Query(request) = match request {
        Ok(request) => request,
        Err(rejection) => {
            return failure(StatusCode::BAD_REQUEST, rejection.to_string());
        }
    };

    match search::execute(reader.as_ref(), request).await {
        Ok(response) => Json(response).into_response(),
        Err(SearchError::InvalidQuery(diagnostic)) => failure(StatusCode::BAD_REQUEST, diagnostic),
        Err(SearchError::Storage(_)) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "food search failed".to_owned(),
        ),
    }
}

#[derive(Debug, Serialize)]
struct SearchFailure {
    error: String,
}

fn failure(status: StatusCode, error: String) -> Response {
    (status, Json(SearchFailure { error })).into_response()
}

#[cfg(test)]
#[path = "../unit-tests/handlers.rs"]
mod tests;
