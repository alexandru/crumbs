use std::sync::Arc;

use crumbs_database::identity::OAuthScope;
use rmcp::handler::server::router::tool::ToolRoute;

pub(crate) struct ToolRegistration<S> {
    pub(super) route: ToolRoute<Arc<S>>,
    pub(super) required_scope: OAuthScope,
}

impl<S> ToolRegistration<S> {
    pub(crate) fn new(route: ToolRoute<Arc<S>>, required_scope: OAuthScope) -> Self {
        Self {
            route,
            required_scope,
        }
    }
}
