use crumbs_database::{FoodId, FoodSearchResult, SearchLimit};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ListRequest {
    #[serde(default)]
    pub(super) limit: SearchLimit,
}

#[derive(Debug, Serialize)]
pub(super) struct ListResponse {
    pub(super) foods: Vec<FoodSearchResult>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(try_from = "i64")]
pub(super) struct FoodIdParameter(FoodId);

impl FoodIdParameter {
    pub(super) const fn food_id(self) -> FoodId {
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
