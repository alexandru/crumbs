use std::error::Error as StdError;

use super::{FoodSearchResults, SearchLimit, SearchQuery, SearchQueryError};

#[test]
fn query_construction_and_deserialization_reject_empty_or_nul_values()
-> Result<(), Box<dyn StdError>> {
    assert_eq!(SearchQuery::new("  "), Err(SearchQueryError::Empty));
    assert_eq!(SearchQuery::new("milk\0cheese"), Err(SearchQueryError::Nul));
    assert!(serde_json::from_str::<SearchQuery>("\" \"").is_err());
    assert!(serde_json::from_str::<SearchQuery>("\"milk\\u0000cheese\"").is_err());
    Ok(())
}

#[test]
fn search_limits_default_and_clamp_to_the_contract_range() -> Result<(), Box<dyn StdError>> {
    assert_eq!(SearchLimit::default().get(), 20);
    for (input, expected) in [
        (i64::MIN, 20),
        (-1, 20),
        (0, 20),
        (1, 20),
        (19, 20),
        (20, 20),
        (1_000, 1_000),
        (1_001, 1_000),
        (i64::MAX, 1_000),
    ] {
        assert_eq!(SearchLimit::from(input).get(), expected, "input {input}");
    }
    for (json, expected) in [("1", 20), ("20", 20), ("1001", 1_000)] {
        let decoded: SearchLimit = serde_json::from_str(json)?;
        assert_eq!(decoded.get(), expected, "input {json}");
    }
    Ok(())
}

#[test]
fn grouped_search_results_serialize_all_empty_groups() -> Result<(), Box<dyn StdError>> {
    let value = serde_json::to_value(FoodSearchResults::default())?;
    assert_eq!(value["common"], serde_json::json!([]));
    assert_eq!(value["branded"], serde_json::json!([]));
    assert_eq!(value["custom"], serde_json::json!([]));
    Ok(())
}
