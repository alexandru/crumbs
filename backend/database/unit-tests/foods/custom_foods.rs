use std::error::Error as StdError;

use crate::foods::*;

#[test]
fn dataset_and_custom_food_input_enforce_their_wire_contracts() -> Result<(), Box<dyn StdError>> {
    assert_eq!(
        serde_json::from_str::<Dataset>("\"common\"")?,
        Dataset::Common
    );
    assert_eq!(
        serde_json::from_str::<Dataset>("\"branded\"")?,
        Dataset::Branded
    );
    assert_eq!(
        serde_json::from_str::<Dataset>("\"custom\"")?,
        Dataset::Custom
    );
    assert!("usda_foundation".parse::<Dataset>().is_err());

    let input: CustomFoodInput = serde_json::from_str(r#"{"description":"  House sauce  "}"#)?;
    assert_eq!(input.description.as_str(), "  House sauce  ");
    assert!(input.category.is_none());
    assert!(input.notes.is_none());
    assert_eq!(
        input.parsed_data,
        crate::nutrition_model::FoodData::default()
    );
    assert!(CustomFoodDescription::new(" \t\n").is_err());
    let blank_description_json = serde_json::to_string(" \t\n")?;
    assert!(serde_json::from_str::<CustomFoodDescription>(&blank_description_json).is_err());
    for invalid_description in ["\0", "\0House sauce", "House\0 sauce"] {
        assert!(CustomFoodDescription::new(invalid_description).is_err());
        let invalid_description_json = serde_json::to_string(invalid_description)?;
        assert!(serde_json::from_str::<CustomFoodDescription>(&invalid_description_json).is_err());
    }

    for forbidden in ["id", "source", "source_food_id", "dataset"] {
        let payload = format!(r#"{{"description":"Valid food","{forbidden}":"custom"}}"#);
        assert!(serde_json::from_str::<CustomFoodInput>(&payload).is_err());
    }
    Ok(())
}
