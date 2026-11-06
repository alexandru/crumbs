use std::borrow::Cow;
use std::error::Error;
use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::{Query, State};
use axum::http::{StatusCode, Uri};
use crumbs_database::{
    Database, DatabaseConfig, Dataset, Food, FoodWriter, SearchLimit, SearchQuery,
};
use serde_json::{Value, json};

use super::*;

#[tokio::test]
async fn root_handler_renders_welcome_page() {
    let markup = hello().await.into_string();

    for text in [
        "Hello, welcome to Crumbs.",
        "A little clarity about what you eat.",
        "Your food library is ready.",
    ] {
        assert!(markup.contains(text), "missing welcome text {text:?}");
    }
    assert!(markup.contains("class=\"crumb crumb-three\""));
    assert!(!markup.contains("<form"));
    assert!(!markup.contains("<a "));
}

#[tokio::test]
async fn search_handler_returns_food_nutrition_and_reports_invalid_fts_syntax()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let database = Arc::new(
        Database::open(DatabaseConfig {
            path: directory.path().join("crumbs.db"),
        })
        .await?,
    );
    let id = database
        .upsert_food(&Food {
            source: Cow::Borrowed("test"),
            source_food_id: "food-1".to_owned(),
            dataset: Dataset::Common,
            description: "Hummus, commercial".to_owned(),
            category: Some("Legumes".to_owned()),
            notes: Some("Chickpeas, tahini, lemon".to_owned()),
            parsed_data: serde_json::from_value(json!({
                "nutrition": {
                    "per": "100 g",
                    "protein": "7 g"
                },
                "serving_sizes": [
                    { "name": "one serving", "quantity": "30 g" }
                ]
            }))?,
        })
        .await?;

    let response = search_foods(
        State(Arc::clone(&database)),
        Ok(Query(SearchRequest {
            query: SearchQuery::new("hummus")?,
            limit: SearchLimit::default(),
        })),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;
    assert_eq!(body["common"].as_array().map(Vec::len), Some(1));
    assert!(body["branded"].as_array().is_some_and(Vec::is_empty));
    assert!(body["custom"].as_array().is_some_and(Vec::is_empty));
    assert_eq!(body["common"][0]["id"], json!(id.0));
    assert_eq!(
        body["common"][0]["notes"],
        json!("Chickpeas, tahini, lemon")
    );
    assert_eq!(
        body["common"][0]["parsed_data"]["nutrition"]["protein"],
        json!("7 g")
    );
    assert_eq!(
        body["common"][0]["parsed_data"]["serving_sizes"][0]["quantity"],
        json!("30 g")
    );

    let response = search_foods(
        State(Arc::clone(&database)),
        Ok(Query(SearchRequest {
            query: SearchQuery::new("tahini")?,
            limit: SearchLimit::default(),
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;
    assert_eq!(body, json!({"common": [], "branded": [], "custom": []}));

    let response = search_foods(
        State(Arc::clone(&database)),
        Ok(Query(SearchRequest {
            query: SearchQuery::new("(milk OR cheese")?,
            limit: SearchLimit::default(),
        })),
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );

    let uri: Uri = "/foods/search?query=".parse()?;
    let response = search_foods(
        State(Arc::clone(&database)),
        Query::<SearchRequest>::try_from_uri(&uri),
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let body: Value = serde_json::from_slice(&body)?;
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );

    database.close().await;
    Ok(())
}
