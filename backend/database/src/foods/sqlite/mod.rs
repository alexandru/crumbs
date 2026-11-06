mod custom_foods;
mod rows;
mod search;
mod source_foods;
mod tags;

use crate::clock::ClockError;

impl From<ClockError> for sqlx::Error {
    fn from(error: ClockError) -> Self {
        Self::Encode(Box::new(error))
    }
}

#[cfg(test)]
#[path = "../../../unit-tests/foods/sqlite/mod.rs"]
mod tests;
