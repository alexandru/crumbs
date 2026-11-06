//! Request-local permissions and HTTP scope challenges for MCP tool calls.

use std::sync::{Arc, OnceLock};

use crumbs_database::oauth::OAuthScope;

#[derive(Clone)]
pub(crate) struct McpAuthorization {
    granted: OAuthScope,
    required: Arc<OnceLock<OAuthScope>>,
}

impl McpAuthorization {
    pub(crate) fn new(granted: OAuthScope) -> Self {
        Self {
            granted,
            required: Arc::new(OnceLock::new()),
        }
    }

    pub(crate) fn permits(&self, required: OAuthScope) -> bool {
        if self.granted.includes(required) {
            return true;
        }
        self.required.get_or_init(|| required);
        false
    }

    pub(crate) fn challenge(&self) -> Option<OAuthScope> {
        self.required.get().copied()
    }
}
