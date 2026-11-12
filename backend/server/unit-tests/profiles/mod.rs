pub(super) mod routes;

use std::io;

use axum::http::StatusCode;
use crumbs_database::profiles::ProfileError;

pub(in crate::profiles) struct ProfileErrorCase {
    pub error: ProfileError,
    pub status: StatusCode,
    pub code: &'static str,
    pub message: &'static str,
}

pub(in crate::profiles) fn profile_error_cases() -> [ProfileErrorCase; 6] {
    [
        ProfileErrorCase {
            error: ProfileError::InvalidInput("invalid profile input".to_owned()),
            status: StatusCode::BAD_REQUEST,
            code: "invalid_input",
            message: "invalid profile input",
        },
        ProfileErrorCase {
            error: ProfileError::DuplicateName,
            status: StatusCode::CONFLICT,
            code: "duplicate_name",
            message: "profile name already exists for this account",
        },
        ProfileErrorCase {
            error: ProfileError::Calculation("calculation detail".to_owned()),
            status: StatusCode::BAD_REQUEST,
            code: "calculation",
            message: "calculation detail",
        },
        ProfileErrorCase {
            error: ProfileError::InvalidStoredData,
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "invalid_stored_data",
            message: "profile operation failed",
        },
        ProfileErrorCase {
            error: ProfileError::Storage(Box::new(io::Error::other(
                "private storage source detail",
            ))),
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "storage",
            message: "profile operation failed",
        },
        ProfileErrorCase {
            error: ProfileError::Clock(Box::new(io::Error::other("private clock source detail"))),
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "clock",
            message: "profile operation failed",
        },
    ]
}

pub(in crate::profiles) fn assert_calorie_balance_goal(
    actual: &serde_json::Value,
    expected: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use crumbs_database::nutrition_model::{Kcal, PerDay};

    let actual = actual
        .as_str()
        .ok_or("missing calorie balance goal")?
        .parse::<PerDay<Kcal>>()?;
    let expected = expected.parse::<PerDay<Kcal>>()?;
    assert_eq!(actual, expected);
    Ok(())
}
