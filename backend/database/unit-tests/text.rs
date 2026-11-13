use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};

use crate::foods::{CustomFoodDescription, SearchQuery, SearchQueryError, TagName};
use crate::profile_logs::{ExerciseType, JournalText, ProfileLogError};
use crate::profiles::{ProfileError, ProfileName};

#[test]
fn exercise_and_journal_text_preserve_input_and_report_blank_errors() -> Result<(), Box<dyn Error>>
{
    let text = "  Café\0\n";
    let exercise = ExerciseType::new(text)?;
    let journal = JournalText::new(text)?;
    assert_eq!(exercise.as_str(), text);
    assert_eq!(journal.as_str(), text);
    assert_eq!(serde_json::to_value(&exercise)?, serde_json::json!(text));
    assert_eq!(serde_json::to_value(&journal)?, serde_json::json!(text));
    assert_eq!(
        serde_json::from_value::<ExerciseType>(serde_json::json!(text))?,
        exercise
    );
    assert_eq!(
        serde_json::from_value::<JournalText>(serde_json::json!(text))?,
        journal
    );
    assert!(
        matches!(ExerciseType::new(" \t"), Err(ProfileLogError::InvalidInput(message)) if message == "exercise type must not be blank")
    );
    assert!(
        matches!(JournalText::new(" \t"), Err(ProfileLogError::InvalidInput(message)) if message == "journal text must not be blank")
    );
    assert!(serde_json::from_value::<ExerciseType>(serde_json::json!(" ")).is_err());
    assert!(serde_json::from_value::<JournalText>(serde_json::json!(" ")).is_err());
    Ok(())
}

#[test]
fn food_text_preserves_content_and_reports_validation_errors() -> Result<(), Box<dyn Error>> {
    let value = "  Café milk  ";
    let description = CustomFoodDescription::new(value)?;
    let query = SearchQuery::new(value)?;
    assert_eq!(description.as_str(), value);
    assert_eq!(description.clone().into_inner(), value);
    assert_eq!(query.to_string(), value);
    assert_eq!(
        serde_json::to_value(&description)?,
        serde_json::json!(value)
    );
    assert_eq!(serde_json::to_value(&query)?, serde_json::json!(value));
    assert_eq!(
        serde_json::from_value::<CustomFoodDescription>(serde_json::json!(value))?,
        description
    );
    assert_eq!(
        serde_json::from_value::<SearchQuery>(serde_json::json!(value))?,
        query
    );
    for value in [" ", "\0", "milk\0cheese"] {
        let error = CustomFoodDescription::new(value)
            .err()
            .ok_or("description accepted")?;
        assert_eq!(
            error.to_string(),
            "custom food description must not be blank or contain a NUL character"
        );
        assert!(serde_json::from_value::<CustomFoodDescription>(serde_json::json!(value)).is_err());
    }
    assert_eq!(SearchQuery::new(" \t"), Err(SearchQueryError::Empty));
    assert_eq!(SearchQuery::new(" \0"), Err(SearchQueryError::Nul));
    assert_eq!(
        SearchQueryError::Empty.to_string(),
        "search query must not be empty"
    );
    assert_eq!(
        SearchQueryError::Nul.to_string(),
        "search query must not contain a NUL character"
    );
    Ok(())
}

#[test]
fn profile_and_tag_names_preserve_spelling_and_hash_canonical_identity()
-> Result<(), Box<dyn Error>> {
    let profile = ProfileName::new("  Straße  ")?;
    let profile_equal = ProfileName::new("STRASSE")?;
    let tag = TagName::new("  Straße  ")?;
    let tag_equal = TagName::new("STRASSE")?;
    assert_eq!(profile.as_str(), "Straße");
    assert_eq!(tag.as_str(), "Straße");
    assert_eq!(profile, profile_equal);
    assert_eq!(tag, tag_equal);
    assert_eq!(hash(&profile), hash(&profile_equal));
    assert_eq!(hash(&tag), hash(&tag_equal));
    assert_ne!(ProfileName::new("é")?, ProfileName::new("e")?);
    assert_ne!(TagName::new("é")?, TagName::new("e")?);
    assert_eq!(serde_json::to_value(&profile)?, serde_json::json!("Straße"));
    assert_eq!(
        serde_json::from_value::<ProfileName>(serde_json::json!("  Straße  "))?.as_str(),
        "Straße"
    );
    assert!(
        matches!(ProfileName::new(" "), Err(ProfileError::InvalidInput(message)) if message == "profile name must not be blank")
    );
    let error = TagName::new(" ").err().ok_or("tag accepted")?;
    assert_eq!(error.to_string(), "tag name must not be blank");
    Ok(())
}

fn hash(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
