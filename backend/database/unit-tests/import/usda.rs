use std::error::Error as StdError;
use std::io;
use std::io::Read;
use std::thread;

use tokio::sync::mpsc;

use super::*;

const FOUNDATION_RECORD: &str = r#"{"fdcId": 321358, "description": "Hummus, commercial", "dataType": "Foundation", "foodCategory": {"description": "Legumes and Legume Products"}, "publicationDate": "4/1/2019", "foodNutrients": [{"nutrient": {"number": "334", "unitName": "µg"}, "amount": 1.00, "ratio": 1e+3}], "scientificName": "Solanum lycopersicum", "unknown": {"nested": [true, null, "µg"]}}"#;
const FOUNDATION_RECORD_NULL_CATEGORY: &str = r#"{"fdcId":321360,"description":"Tomatoes, grape, raw","dataType":"Foundation","foodCategory":null}"#;
const BRANDED_RECORD: &str = r#"{"foodClass":"Branded","description":"GRANOLA, CINNAMON, RAISIN","foodNutrients":[],"publicationDate":"11/13/2020","brandOwner":"MICHELE'S","gtinUpc":"001633636543505","dataSource":"LI","servingSize":28,"servingSizeUnit":"g","householdServingFullText":"0.25 cup","brandedFoodCategory":"Cereal","modifiedDate":"4/26/2020","availableDate":"4/26/2020","dataType":"Branded","fdcId":1106281,"labelNutrients":{"calories":{"value":120}}}"#;
const SURVEY_RECORD: &str = r#"{"fdcId":2705383,"description":"Milk, human","dataType":"Survey (FNDDS)","wweiaFoodCategory":{"wweiaFoodCategoryDescription":"Human milk","wweiaFoodCategoryCode":3298314}}"#;
const IO_FAILURE_FIRST_RECORD: &str =
    r#"{"fdcId":1,"description":"First record","dataType":"Foundation"}"#;
const IO_FAILURE_SECOND_RECORD: &str =
    r#"{"fdcId":2,"description":"Second record","dataType":"Foundation"}"#;

#[test]
fn preserves_survey_wweia_category() -> Result<(), Box<dyn StdError>> {
    let input = document("SurveyFoods", &[SURVEY_RECORD]);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;

    let food = first_food(&foods)?;
    assert_eq!(
        food.category.as_ref().map(FoodCategory::as_str),
        Some("Human milk")
    );
    assert_eq!(food.raw.get(), SURVEY_RECORD);
    Ok(())
}

#[test]
fn survey_category_fallback_preserves_existing_precedence() -> Result<(), Box<dyn StdError>> {
    let records = [
        (
            r#"{"fdcId":9,"description":"Food","dataType":"Survey (FNDDS)","wweiaFoodCategory":["Human milk"]}"#,
            None,
        ),
        (
            r#"{"fdcId":1,"description":"Food","dataType":"Survey (FNDDS)"}"#,
            None,
        ),
        (
            r#"{"fdcId":2,"description":"Food","dataType":"Survey (FNDDS)","foodCategory":null,"wweiaFoodCategory":null}"#,
            None,
        ),
        (
            r#"{"fdcId":3,"description":"Food","dataType":"Survey (FNDDS)","foodCategory":{"description":null},"wweiaFoodCategory":{"wweiaFoodCategoryDescription":"WWEIA"}}"#,
            Some("WWEIA"),
        ),
        (
            r#"{"fdcId":4,"description":"Food","dataType":"Survey (FNDDS)","foodCategory":{"description":"First","description":"Legacy"},"wweiaFoodCategory":{"wweiaFoodCategoryDescription":"WWEIA"}}"#,
            Some("Legacy"),
        ),
        (
            r#"{"fdcId":5,"description":"Food","dataType":"Survey (FNDDS)","wweiaFoodCategory":"invalid"}"#,
            None,
        ),
        (
            r#"{"fdcId":6,"description":"Food","dataType":"Survey (FNDDS)","wweiaFoodCategory":{"wweiaFoodCategoryDescription":42}}"#,
            None,
        ),
        (
            r#"{"fdcId":7,"description":"Food","dataType":"Survey (FNDDS)","wweiaFoodCategory":null,"wweiaFoodCategory":{"wweiaFoodCategoryDescription":"WWEIA"}}"#,
            None,
        ),
        (
            r#"{"fdcId":8,"description":"Food","dataType":"Survey (FNDDS)","wweiaFoodCategory":{"wweiaFoodCategoryDescription":"First","wweiaFoodCategoryDescription":"Second"}}"#,
            Some("Second"),
        ),
    ];

    for (record, expected) in records {
        let input = document("SurveyFoods", &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        result?;
        assert_eq!(
            first_food(&foods)?
                .category
                .as_ref()
                .map(FoodCategory::as_str),
            expected
        );
    }
    Ok(())
}

#[test]
fn ignores_wweia_metadata_for_other_food_kinds() -> Result<(), Box<dyn StdError>> {
    let cases = [
        (
            "FoundationFoods",
            r#"{"fdcId":1,"description":"Food","dataType":"Foundation","wweiaFoodCategory":42}"#,
        ),
        (
            "SRLegacyFoods",
            r#"{"fdcId":2,"description":"Food","dataType":"SR Legacy","wweiaFoodCategory":{"wweiaFoodCategoryDescription":"WWEIA"}}"#,
        ),
        (
            "BrandedFoods",
            r#"{"fdcId":3,"description":"Food","dataType":"Branded","wweiaFoodCategory":null,"wweiaFoodCategory":"invalid"}"#,
        ),
    ];

    for (root, record) in cases {
        let input = document(root, &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        result?;
        assert_eq!(first_food(&foods)?.category, None);
    }
    Ok(())
}

#[test]
fn streams_foundation_foods_and_preserves_raw_payload() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[FOUNDATION_RECORD, FOUNDATION_RECORD_NULL_CATEGORY],
    );

    let (result, foods) = collect(input.as_bytes(), 1);
    result?;

    assert_eq!(foods.len(), 2);
    let first = first_food(&foods)?;
    assert_eq!(first.fdc_id.as_str(), "321358");
    assert_eq!(first.kind, FoodKind::Foundation);
    assert_eq!(first.description.as_str(), "Hummus, commercial");
    assert_eq!(
        first.category.as_ref().map(FoodCategory::as_str),
        Some("Legumes and Legume Products")
    );
    assert_eq!(first.position.get(), 1);
    assert_eq!(first.raw.get(), FOUNDATION_RECORD);

    let second = foods
        .get(1)
        .ok_or_else(|| io::Error::other("missing second food"))?;
    assert_eq!(second.fdc_id.as_str(), "321360");
    assert_eq!(second.kind, FoodKind::Foundation);
    assert_eq!(second.category, None);
    assert_eq!(second.position.get(), 2);
    assert_eq!(second.raw.get(), FOUNDATION_RECORD_NULL_CATEGORY);
    Ok(())
}

#[test]
fn streams_branded_food_and_preserves_gtin_text() -> Result<(), Box<dyn StdError>> {
    let input = document("BrandedFoods", &[BRANDED_RECORD]);

    let (result, foods) = collect(input.as_bytes(), 1);
    result?;

    assert_eq!(foods.len(), 1);
    let food = first_food(&foods)?;
    assert_eq!(food.fdc_id.as_str(), "1106281");
    assert_eq!(food.kind, FoodKind::Branded);
    assert_eq!(food.description.as_str(), "GRANOLA, CINNAMON, RAISIN");
    assert_eq!(
        food.category.as_ref().map(FoodCategory::as_str),
        Some("Cereal")
    );
    assert_eq!(food.raw.get(), BRANDED_RECORD);
    assert!(
        food.raw.get().contains(r#""gtinUpc":"001633636543505""#),
        "gtinUpc was not preserved as text: {}",
        food.raw.get()
    );
    Ok(())
}

#[test]
fn accepts_sr_legacy_and_survey_roots() -> Result<(), Box<dyn StdError>> {
    let sr_input = document(
        "SRLegacyFoods",
        &[r#"{"fdcId":1,"description":"Legacy food","dataType":"SR Legacy"}"#],
    );
    let (sr_result, sr_foods) = collect(sr_input.as_bytes(), 1);
    sr_result?;
    let sr_food = first_food(&sr_foods)?;
    assert_eq!(sr_food.kind, FoodKind::SrLegacy);

    let survey_input = document(
        "SurveyFoods",
        &[r#"{"fdcId":2,"description":"Survey food","dataType":"Survey (FNDDS)"}"#],
    );
    let (survey_result, survey_foods) = collect(survey_input.as_bytes(), 1);
    survey_result?;
    let survey_food = first_food(&survey_foods)?;
    assert_eq!(survey_food.kind, FoodKind::Survey);
    Ok(())
}

#[test]
fn allows_missing_and_null_categories() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[
            r#"{"fdcId":1,"description":"No category","dataType":"Foundation"}"#,
            r#"{"fdcId":2,"description":"Null category","dataType":"Foundation","foodCategory":null}"#,
        ],
    );

    let (result, foods) = collect(input.as_bytes(), 1);
    result?;

    assert_eq!(foods.len(), 2);
    for food in &foods {
        assert_eq!(food.category, None);
    }
    Ok(())
}

#[test]
fn accepts_empty_recognized_array() -> Result<(), Box<dyn StdError>> {
    let (result, foods) = collect(br#"{"FoundationFoods":[]}"#, 1);
    result?;
    assert!(foods.is_empty());
    Ok(())
}

#[test]
fn accepts_trailing_whitespace() -> Result<(), Box<dyn StdError>> {
    let (result, foods) = collect(b"{\"FoundationFoods\":[]}\n\t ", 1);
    result?;
    assert!(foods.is_empty());
    Ok(())
}

#[test]
fn rejects_empty_input_and_empty_object() -> Result<(), Box<dyn StdError>> {
    for input in [&b""[..], &b"{}"[..]] {
        let (result, foods) = collect(input, 1);
        assert!(foods.is_empty());
        let error = error(result)?;
        assert!(
            matches!(&error, ParseError::Document { .. }),
            "unexpected error: {error}"
        );
    }
    Ok(())
}

#[test]
fn rejects_unknown_multiple_and_trailing_documents() -> Result<(), Box<dyn StdError>> {
    let cases = [
        (
            &br#"{"OtherFoods":[]}"#[..],
            "unsupported root",
            "OtherFoods",
        ),
        (
            &br#"{"FoundationFoods":[],"BrandedFoods":[]}"#[..],
            "multiple roots",
            "another entry",
        ),
        (
            &br#"{"FoundationFoods":[]} trailing"#[..],
            "trailing content",
            "trailing",
        ),
        (
            &br#"{"FoundationFoods":{}}"#[..],
            "non-array root",
            "invalid type",
        ),
    ];

    for (input, label, message) in cases {
        let (result, foods) = collect(input, 1);
        assert!(foods.is_empty(), "{label} produced food records");
        let error = error(result)?;
        assert!(
            matches!(&error, ParseError::Document { .. }),
            "{label} produced the wrong error: {error}"
        );
        assert!(
            error.to_string().contains(message),
            "{label} message did not contain {message:?}: {error}"
        );
    }
    Ok(())
}

#[test]
fn rejects_malformed_json() -> Result<(), Box<dyn StdError>> {
    let (result, foods) = collect(br#"{"FoundationFoods":[{"fdcId":1,}]}"#, 1);
    assert!(foods.is_empty());
    let error = error(result)?;
    match error {
        ParseError::Record {
            position,
            source: RecordError::Syntax(source),
        } => {
            assert_eq!(position.get(), 1);
            assert!(source.line() > 0);
            assert!(source.column() > 0);
        }
        other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
    Ok(())
}

#[test]
fn reports_io_failure_while_reading_a_later_record() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[IO_FAILURE_FIRST_RECORD, IO_FAILURE_SECOND_RECORD],
    );
    let second_start = record_offset(&input, IO_FAILURE_SECOND_RECORD)?;
    let fail_after = second_start + IO_FAILURE_SECOND_RECORD.len() / 2;
    let reader = FailingReader::new(input.as_bytes(), fail_after);

    let (result, foods) = collect_reader(reader, 1);

    assert_eq!(
        foods.len(),
        1,
        "the valid preceding record was not delivered"
    );
    let delivered = first_food(&foods)?;
    assert_eq!(delivered.fdc_id.as_str(), "1");

    let error = error(result)?;
    let displayed = error.to_string();
    assert!(
        displayed.contains("failed to read record JSON: simulated input read failure"),
        "unexpected read error label: {displayed}"
    );
    assert!(
        !displayed.contains("malformed record JSON"),
        "I/O failure was labeled as malformed JSON: {displayed}"
    );
    match error {
        ParseError::Record {
            position,
            source: RecordError::Read(source),
        } => {
            assert_eq!(position.get(), 2);
            assert_eq!(source.io_error_kind(), Some(io::ErrorKind::Other));
            assert!(source.to_string().contains("simulated input read failure"));
        }
        other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
    Ok(())
}

#[test]
fn reports_io_failure_while_reading_document_framing() -> Result<(), Box<dyn StdError>> {
    let input = br#"{"FoundationFoods":["#;
    let reader = FailingReader::new(input, 5);

    let (result, foods) = collect_reader(reader, 1);

    assert!(foods.is_empty());
    let error = error(result)?;
    require_document_read_error(&error)?;
    Ok(())
}

#[test]
fn reports_io_failure_while_checking_trailing_input() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[IO_FAILURE_FIRST_RECORD, IO_FAILURE_SECOND_RECORD],
    );
    let reader = FailingReader::new(input.as_bytes(), input.len());

    let (result, foods) = collect_reader(reader, 1);

    assert_eq!(foods.len(), 2, "completed records were not delivered");
    let first = first_food(&foods)?;
    assert_eq!(first.fdc_id.as_str(), "1");
    let second = foods
        .get(1)
        .ok_or_else(|| io::Error::other("second food is missing"))?;
    assert_eq!(second.fdc_id.as_str(), "2");

    let error = error(result)?;
    require_document_read_error(&error)?;
    Ok(())
}

#[test]
fn rejects_missing_header_fields_with_json_position() -> Result<(), Box<dyn StdError>> {
    let records = [
        r#"{"description":"Food","dataType":"Foundation"}"#,
        r#"{"fdcId":1,"dataType":"Foundation"}"#,
        r#"{"fdcId":1,"description":"Food"}"#,
    ];

    for record in records {
        let input = document("FoundationFoods", &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        assert!(foods.is_empty());
        let error = error(result)?;
        match error {
            ParseError::Record {
                position,
                source: RecordError::Header(source),
            } => {
                assert_eq!(position.get(), 1);
                assert!(source.line() > 0);
                assert!(source.column() > 0);
            }
            other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
        }
    }
    Ok(())
}

#[test]
fn rejects_wrong_header_field_types() -> Result<(), Box<dyn StdError>> {
    let records = [
        r#"{"fdcId":"1","description":"Food","dataType":"Foundation"}"#,
        r#"{"fdcId":-1,"description":"Food","dataType":"Foundation"}"#,
        r#"{"fdcId":1.5,"description":"Food","dataType":"Foundation"}"#,
        r#"{"fdcId":1,"description":42,"dataType":"Foundation"}"#,
        r#"{"fdcId":1,"description":"Food","dataType":42}"#,
        r#"{"fdcId":1,"description":"Food","dataType":"Sample"}"#,
    ];

    for record in records {
        let input = document("FoundationFoods", &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        assert!(foods.is_empty());
        let error = error(result)?;
        assert!(
            matches!(
                error,
                ParseError::Record {
                    source: RecordError::Header(_),
                    ..
                }
            ),
            "wrong-type record did not produce a header error"
        );
    }
    Ok(())
}

#[test]
fn rejects_positional_array_record() -> Result<(), Box<dyn StdError>> {
    let input = document("FoundationFoods", &[r#"[123,"Apple","Foundation"]"#]);
    let (result, foods) = collect(input.as_bytes(), 1);
    assert!(foods.is_empty());
    let error = error(result)?;

    assert!(
        matches!(
            error,
            ParseError::Record {
                position,
                source: RecordError::NotObject,
            } if position.get() == 1
        ),
        "positional array was accepted as a food record"
    );
    Ok(())
}

#[test]
fn rejects_zero_fdc_id() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[r#"{"fdcId":0,"description":"Food","dataType":"Foundation"}"#],
    );
    let (result, foods) = collect(input.as_bytes(), 1);
    assert!(foods.is_empty());
    let error = error(result)?;

    assert!(
        matches!(
            error,
            ParseError::Record {
                position,
                source: RecordError::ZeroFdcId,
            } if position.get() == 1
        ),
        "zero fdcId was not rejected"
    );
    Ok(())
}

#[test]
fn rejects_blank_description_with_record_position() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[
            FOUNDATION_RECORD_NULL_CATEGORY,
            r#"{"fdcId":3,"description":" \t ","dataType":"Foundation"}"#,
        ],
    );
    let (result, foods) = collect(input.as_bytes(), 1);
    assert_eq!(foods.len(), 1);
    let error = error(result)?;

    assert!(
        matches!(
            error,
            ParseError::Record {
                position,
                source: RecordError::BlankDescription,
            } if position.get() == 2
        ),
        "blank description was not rejected at record two"
    );
    Ok(())
}

#[test]
fn rejects_root_and_data_type_mismatch() -> Result<(), Box<dyn StdError>> {
    let input = document(
        "FoundationFoods",
        &[r#"{"fdcId":1,"description":"Food","dataType":"Branded"}"#],
    );
    let (result, foods) = collect(input.as_bytes(), 1);
    assert!(foods.is_empty());
    let error = error(result)?;

    match error {
        ParseError::Record {
            position,
            source: RecordError::KindMismatch { root, data_type },
        } => {
            assert_eq!(position.get(), 1);
            assert_eq!(root, FoodKind::Foundation);
            assert_eq!(data_type, FoodKind::Branded);
        }
        other => return Err(io::Error::other(format!("unexpected error: {other}")).into()),
    }
    Ok(())
}

#[test]
fn rejects_malformed_category_types() -> Result<(), Box<dyn StdError>> {
    let cases = [
        (
            "FoundationFoods",
            r#"{"fdcId":1,"description":"Food","dataType":"Foundation","foodCategory":"Legumes"}"#,
        ),
        (
            "FoundationFoods",
            r#"{"fdcId":1,"description":"Food","dataType":"Foundation","foodCategory":[]}"#,
        ),
        (
            "FoundationFoods",
            r#"{"fdcId":1,"description":"Food","dataType":"Foundation","foodCategory":{"description":42}}"#,
        ),
        (
            "BrandedFoods",
            r#"{"fdcId":1,"description":"Food","dataType":"Branded","brandedFoodCategory":42}"#,
        ),
    ];

    for (root, record) in cases {
        let input = document(root, &[record]);
        let (result, foods) = collect(input.as_bytes(), 1);
        assert!(foods.is_empty(), "record was accepted: {record}");
        let error = error(result)?;
        assert!(
            matches!(
                error,
                ParseError::Record {
                    source: RecordError::Header(_),
                    ..
                }
            ),
            "malformed category did not produce a header error"
        );
    }
    Ok(())
}

#[test]
fn reports_consumer_disconnect() -> Result<(), Box<dyn StdError>> {
    let input = document("FoundationFoods", &[FOUNDATION_RECORD_NULL_CATEGORY]);
    let (sender, receiver) = mpsc::channel::<PreparedFood>(1);
    drop(receiver);

    let result = stream_foods(input.as_bytes(), &sender);
    let error = error(result)?;

    assert!(
        matches!(
            error,
            ParseError::ConsumerDisconnected { position } if position.get() == 1
        ),
        "consumer disconnect was not reported"
    );
    Ok(())
}

#[test]
fn streams_large_input_to_incremental_consumer() -> Result<(), Box<dyn StdError>> {
    let input = generated_foundation_json(5_000);
    let (sender, mut receiver) = mpsc::channel::<PreparedFood>(1);

    let consumer = thread::spawn(move || {
        let first = receiver.blocking_recv();
        drop(receiver);
        first
    });

    let mut reader = CountingReader::new(input.as_bytes());
    let result = stream_foods(&mut reader, &sender);
    drop(sender);

    let consumer_result = consumer.join();
    assert!(consumer_result.is_ok(), "consumer thread panicked");
    let first = consumer_result.ok().flatten();
    assert!(first.is_some(), "consumer received no record");
    let bytes_read = reader.bytes_read();

    let error = error(result)?;
    assert!(
        matches!(error, ParseError::ConsumerDisconnected { .. }),
        "early receiver drop was not reported"
    );
    assert!(
        bytes_read < input.len() / 2,
        "parsed {bytes_read} of {} bytes before the receiver disappeared",
        input.len()
    );
    Ok(())
}

#[test]
fn streams_all_records_from_large_input() -> Result<(), Box<dyn StdError>> {
    let input = generated_foundation_json(2_000);
    let (result, foods) = collect(input.as_bytes(), 1);
    result?;

    assert_eq!(foods.len(), 2_000);
    let last = foods
        .last()
        .ok_or_else(|| io::Error::other("missing last food"))?;
    assert_eq!(last.position.get(), 2_000);
    assert_eq!(last.fdc_id.as_str(), "2000");
    Ok(())
}

fn document(root: &str, records: &[&str]) -> String {
    let mut input = String::from("{\"");
    input.push_str(root);
    input.push_str("\":[");
    input.push_str(&records.join(","));
    input.push_str("]}");
    input
}

fn collect(input: &[u8], capacity: usize) -> (Result<(), ParseError>, Vec<PreparedFood>) {
    collect_reader(input, capacity)
}

fn collect_reader(
    input: impl Read,
    capacity: usize,
) -> (Result<(), ParseError>, Vec<PreparedFood>) {
    let (sender, mut receiver) = mpsc::channel(capacity);
    let consumer = thread::spawn(move || {
        let mut foods = Vec::new();
        while let Some(food) = receiver.blocking_recv() {
            foods.push(food);
        }
        foods
    });

    let result = stream_foods(input, &sender);
    drop(sender);

    let consumer_result = consumer.join();
    assert!(consumer_result.is_ok(), "consumer thread panicked");
    let foods = consumer_result.unwrap_or_default();
    (result, foods)
}

fn record_offset(input: &str, record: &str) -> Result<usize, io::Error> {
    input
        .find(record)
        .ok_or_else(|| io::Error::other("record is missing from the test document"))
}

fn error(result: Result<(), ParseError>) -> Result<ParseError, io::Error> {
    result
        .err()
        .ok_or_else(|| io::Error::other("input was accepted"))
}

fn require_document_read_error(error: &ParseError) -> Result<(), io::Error> {
    let displayed = error.to_string();
    if displayed.contains("invalid USDA FoodData Central document") {
        return Err(io::Error::other(format!(
            "I/O failure was labeled as an invalid document: {displayed}"
        )));
    }
    if !displayed.contains("failed to read USDA FoodData Central document:") {
        return Err(io::Error::other(format!(
            "unexpected document read error label: {displayed}"
        )));
    }
    match error {
        ParseError::Read { source } => {
            let kind = source.io_error_kind();
            if kind != Some(io::ErrorKind::Other) {
                return Err(io::Error::other(format!(
                    "unexpected I/O error kind: {kind:?}"
                )));
            }
            if !source.to_string().contains("simulated input read failure") {
                return Err(io::Error::other(format!(
                    "unexpected I/O error message: {source}"
                )));
            }
            Ok(())
        }
        other => Err(io::Error::other(format!("unexpected error: {other}"))),
    }
}

fn first_food(foods: &[PreparedFood]) -> Result<&PreparedFood, io::Error> {
    foods
        .first()
        .ok_or_else(|| io::Error::other("no food records"))
}

fn generated_foundation_json(count: usize) -> String {
    let mut json = String::from("{\"FoundationFoods\":[");
    for id in 1..=count {
        if id > 1 {
            json.push(',');
        }
        json.push_str("{\"fdcId\":");
        json.push_str(&id.to_string());
        json.push_str(",\"description\":\"Food ");
        json.push_str(&id.to_string());
        json.push_str("\",\"dataType\":\"Foundation\",\"foodCategory\":{\"description\":\"Test\"},\"foodNutrients\":[{\"amount\":1.00}]}");
    }
    json.push_str("]}");
    json
}

struct CountingReader<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> CountingReader<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    const fn bytes_read(&self) -> usize {
        self.offset
    }
}

impl Read for CountingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = match self.input.get(self.offset..) {
            Some(remaining) => remaining,
            None => &[],
        };
        let count = remaining.len().min(buffer.len());
        let source = remaining
            .get(..count)
            .ok_or_else(|| io::Error::other("source range was out of bounds"))?;
        let destination = buffer
            .get_mut(..count)
            .ok_or_else(|| io::Error::other("buffer range was out of bounds"))?;
        destination.copy_from_slice(source);
        self.offset = self
            .offset
            .checked_add(count)
            .ok_or_else(|| io::Error::other("read offset overflowed"))?;
        Ok(count)
    }
}

struct FailingReader<'a> {
    input: &'a [u8],
    offset: usize,
    fail_after: usize,
}

impl<'a> FailingReader<'a> {
    const fn new(input: &'a [u8], fail_after: usize) -> Self {
        Self {
            input,
            offset: 0,
            fail_after,
        }
    }
}

impl Read for FailingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.offset >= self.fail_after {
            return Err(io::Error::other("simulated input read failure"));
        }
        let remaining = match self.input.get(self.offset..) {
            Some(remaining) => remaining,
            None => &[],
        };
        let available = self.fail_after - self.offset;
        let count = remaining.len().min(buffer.len()).min(available);
        let source = remaining
            .get(..count)
            .ok_or_else(|| io::Error::other("source range was out of bounds"))?;
        let destination = buffer
            .get_mut(..count)
            .ok_or_else(|| io::Error::other("buffer range was out of bounds"))?;
        destination.copy_from_slice(source);
        self.offset = self
            .offset
            .checked_add(count)
            .ok_or_else(|| io::Error::other("read offset overflowed"))?;
        Ok(count)
    }
}
