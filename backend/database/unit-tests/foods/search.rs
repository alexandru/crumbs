use std::error::Error as StdError;

use super::{SearchQuery, SearchQueryError, SearchResults};

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
fn grouped_search_results_serialize_all_empty_groups() -> Result<(), Box<dyn StdError>> {
    let value = serde_json::to_value(SearchResults::default())?;
    assert_eq!(value["common"], serde_json::json!([]));
    assert_eq!(value["branded"], serde_json::json!([]));
    assert_eq!(value["custom"], serde_json::json!([]));
    Ok(())
}
