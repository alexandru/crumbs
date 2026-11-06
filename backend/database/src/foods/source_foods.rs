//! Contracts for source-supplied food writes and lookup.

use std::error::Error as StdError;
use std::future::Future;

use super::model::{Food, FoodId, FoodSummary, SourceFoodIdRef, SourceRef};

/// Persists source-scoped foods independently of their import format.
pub trait FoodWriter: Sync {
    /// Failure reported by this storage implementation.
    type Error: StdError + Send + Sync + 'static;

    /// Atomically inserts a complete source-supplied food or replaces its source-supplied fields,
    /// preserving its local ID and tags.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if the food cannot be persisted.
    fn upsert_food(&self, food: &Food) -> impl Future<Output = Result<FoodId, Self::Error>> + Send;
}

/// Reads food metadata by its source-scoped identity.
pub trait FoodLookup: Sync {
    /// Failure reported by this storage implementation.
    type Error: StdError + Send + Sync + 'static;

    /// Looks up food metadata by its exact source-scoped identity.
    /// The input values are compared as provided, without normalization.
    /// Returns `None` if the identity is absent.
    ///
    /// # Errors
    ///
    /// Returns an implementation error if storage access fails.
    fn food_by_source(
        &self,
        source: SourceRef<'_>,
        source_food_id: SourceFoodIdRef<'_>,
    ) -> impl Future<Output = Result<Option<FoodSummary>, Self::Error>> + Send;
}
