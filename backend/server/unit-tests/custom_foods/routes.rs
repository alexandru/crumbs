use std::error::Error;

use axum::http::{StatusCode, header};
use crumbs_database::nutrition::FoodData;
use crumbs_database::{Dataset, Food, FoodId, FoodWriter, Source, TagName};
use serde_json::{Value, json};

use crate::oauth::tests::{TEST_REST_ACCESS_TOKEN, TEST_REST_WRITE_ACCESS_TOKEN};
use crate::tests::{TestApplication, http_json_request, http_method_request, http_request};

#[tokio::test]
async fn rest_custom_foods_support_full_crud_and_search_notes_are_not_indexed()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    let input = json!({
        "description": "My mozzarella toast",
        "category": "Breakfast",
        "notes": "Mozzarella, basil, and shallot",
        "parsed_data": {
            "nutrition": { "per": "100 g", "energy": "280 kcal", "protein": "14 g" },
            "serving_sizes": [{ "name": "one slice", "quantity": "90 g" }]
        }
    });
    let created = app
        .request(http_json_request(
            "POST",
            "/foods/custom",
            TEST_REST_WRITE_ACCESS_TOKEN,
            input,
        )?)
        .await?;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.json);
    let id = created.json["id"]
        .as_i64()
        .ok_or("created food has no ID")?;
    assert_eq!(created.json["source"], "custom");
    assert_eq!(created.json["dataset"], "custom");
    assert_eq!(created.json["notes"], "Mozzarella, basil, and shallot");
    assert_eq!(created.json["parsed_data"]["nutrition"]["protein"], "14 g");
    assert_eq!(
        created
            .headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("/foods/custom/{id}").as_str())
    );
    app.database
        .set_food_tags(FoodId(id), &[TagName::new("dietary")?])
        .await?;

    let item_uri = format!("/foods/custom/{id}");
    let fetched = app.request(http_request(&item_uri)?).await?;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.json, created.json);

    let listed = app.request(http_request("/foods/custom?limit=20")?).await?;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json["foods"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed.json["foods"][0], created.json);

    let visible = app
        .request(http_request("/foods/search?query=mozzarella")?)
        .await?;
    assert_eq!(visible.status, StatusCode::OK);
    assert_eq!(visible.json["custom"].as_array().map(Vec::len), Some(1));
    assert_eq!(visible.json["custom"][0]["notes"], created.json["notes"]);
    let notes_only = app
        .request(http_request("/foods/search?query=shallot")?)
        .await?;
    assert_eq!(
        notes_only.json,
        json!({ "common": [], "branded": [], "custom": [] })
    );

    let replaced = app
        .request(http_json_request(
            "PUT",
            &item_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({ "description": "Updated cheese toast" }),
        )?)
        .await?;
    assert_eq!(replaced.status, StatusCode::OK, "{:?}", replaced.json);
    assert_eq!(replaced.json["id"], json!(id));
    assert_eq!(replaced.json["description"], "Updated cheese toast");
    assert_eq!(replaced.json["category"], Value::Null);
    assert_eq!(replaced.json["notes"], Value::Null);
    assert_eq!(replaced.json["parsed_data"]["nutrition"], Value::Null);
    let old_name = app
        .request(http_request("/foods/search?query=mozzarella")?)
        .await?;
    assert_eq!(old_name.json["custom"].as_array().map(Vec::len), Some(0));
    let new_name = app
        .request(http_request("/foods/search?query=updated")?)
        .await?;
    assert_eq!(new_name.json["custom"].as_array().map(Vec::len), Some(1));
    let retained_tag = app
        .request(http_request("/foods/search?query=tags%3Adietary")?)
        .await?;
    assert_eq!(
        retained_tag.json["custom"].as_array().map(Vec::len),
        Some(1)
    );

    let deleted = app
        .request(http_method_request(
            "DELETE",
            &item_uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
        )?)
        .await?;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert_eq!(deleted.json, Value::Null);
    let missing = app.request(http_request(&item_uri)?).await?;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    Ok(())
}

#[tokio::test]
async fn rest_custom_foods_reject_invalid_input_and_protect_imported_foods()
-> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    for input in [
        json!({}),
        json!({"description":"   "}),
        json!({
            "description":"Valid name",
            "source":"usda"
        }),
    ] {
        let response = app
            .request(http_json_request(
                "POST",
                "/foods/custom",
                TEST_REST_WRITE_ACCESS_TOKEN,
                input,
            )?)
            .await?;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{:?}",
            response.json
        );
    }

    let imported_id = app
        .database
        .upsert_food(&Food {
            source: Source::Borrowed("usda"),
            source_food_id: "protected-import".to_owned(),
            dataset: Dataset::Common,
            description: "Imported oats".to_owned(),
            category: None,
            notes: None,
            parsed_data: FoodData::default(),
        })
        .await?;
    let uri = format!("/foods/custom/{}", imported_id.0);
    let update = app
        .request(http_json_request(
            "PUT",
            &uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"description":"Hijacked oats"}),
        )?)
        .await?;
    assert_eq!(update.status, StatusCode::NOT_FOUND);
    let delete = app
        .request(http_method_request(
            "DELETE",
            &uri,
            TEST_REST_WRITE_ACCESS_TOKEN,
        )?)
        .await?;
    assert_eq!(delete.status, StatusCode::NOT_FOUND);

    let invalid_id = app
        .request(http_request("/foods/custom/not-an-id")?)
        .await?;
    assert_eq!(invalid_id.status, StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn rest_custom_food_writes_require_write_scope() -> Result<(), Box<dyn Error>> {
    let app = TestApplication::new().await?;
    let created = app
        .request(http_json_request(
            "POST",
            "/foods/custom",
            TEST_REST_WRITE_ACCESS_TOKEN,
            json!({"description":"Permission check food"}),
        )?)
        .await?;
    let id = created.json["id"]
        .as_i64()
        .ok_or("created food has no ID")?;
    let uri = format!("/foods/custom/{id}");

    let create = app
        .request(http_json_request(
            "POST",
            "/foods/custom",
            TEST_REST_ACCESS_TOKEN,
            json!({"description":"Read-only create"}),
        )?)
        .await?;
    assert_eq!(create.status, StatusCode::FORBIDDEN);
    let update = app
        .request(http_json_request(
            "PUT",
            &uri,
            TEST_REST_ACCESS_TOKEN,
            json!({"description":"Read-only update"}),
        )?)
        .await?;
    assert_eq!(update.status, StatusCode::FORBIDDEN);
    let deleted = app
        .request(http_method_request("DELETE", &uri, TEST_REST_ACCESS_TOKEN)?)
        .await?;
    assert_eq!(deleted.status, StatusCode::FORBIDDEN);
    Ok(())
}
