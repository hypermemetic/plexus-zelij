use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use std::sync::Arc;

use crate::backend::TerminalBackend;
use crate::plexus::{Activation, ChildRouter, PlexusError, PlexusStream};
use crate::types::{LocusEvent, SessionOpts};

/// Sessions sub-activation — manages terminal sessions.
///
/// Accessed as `locus.sessions.list`, `locus.sessions.create`, etc.
#[derive(Clone)]
pub struct SessionsActivation {
    /// Terminal backend instance shared across all activations
    pub(crate) backend: Arc<dyn TerminalBackend>,
}

impl SessionsActivation {
    /// Create a new SessionsActivation with the specified backend
    pub fn new(backend: Arc<dyn TerminalBackend>) -> Self {
        Self { backend }
    }
}

#[allow(missing_docs)]
#[plexus_macros::hub_methods(
    namespace = "sessions",
    version = "0.1.0",
    description = "Terminal session management"
)]
impl SessionsActivation {
    #[plexus_macros::hub_method(description = "List all terminal sessions")]
    async fn list(&self) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.list_sessions().await {
                Ok(sessions) => yield LocusEvent::Sessions { sessions },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Create a new terminal session",
        params(
            name = "Session name",
            layout = "Optional layout file path",
            cwd = "Working directory"
        )
    )]
    async fn create(
        &self,
        name: String,
        layout: Option<String>,
        cwd: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let opts = SessionOpts {
                name,
                layout,
                cwd: cwd.map(Into::into),
            };
            match backend.create_session(&opts).await {
                Ok(session) => yield LocusEvent::SessionCreated { session },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Kill a terminal session",
        params(name = "Session name to kill")
    )]
    async fn kill(&self, name: String) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.kill_session(&name).await {
                Ok(()) => yield LocusEvent::Ok { message: format!("Killed session: {name}") },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }
}

#[async_trait]
impl ChildRouter for SessionsActivation {
    fn router_namespace(&self) -> &'static str {
        "sessions"
    }

    async fn router_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, _name: &str) -> Option<Box<dyn ChildRouter>> {
        None
    }
}
