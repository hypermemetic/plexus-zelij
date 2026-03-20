use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use std::sync::Arc;

use crate::backend::TerminalBackend;
use crate::plexus::{Activation, ChildRouter, PlexusError, PlexusStream};
use crate::types::LocusEvent;

/// Info sub-activation — backend status and layout queries.
///
/// Accessed as `locus.info.status`, `locus.info.layout`.
#[derive(Clone)]
pub struct InfoActivation {
    /// Terminal backend instance shared across all activations
    pub(crate) backend: Arc<dyn TerminalBackend>,
}

impl InfoActivation {
    /// Create a new InfoActivation with the specified backend
    pub fn new(backend: Arc<dyn TerminalBackend>) -> Self {
        Self { backend }
    }
}

#[allow(missing_docs)]
#[plexus_macros::hub_methods(
    namespace = "info",
    version = "0.1.0",
    description = "Backend status and layout info"
)]
impl InfoActivation {
    #[plexus_macros::hub_method(
        description = "Check which terminal backend is active and if it's available"
    )]
    async fn status(&self) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let available = backend.is_available().await;
            let name = backend.name().to_string();
            if available {
                yield LocusEvent::Ok { message: format!("Backend '{name}' is available") };
            } else {
                yield LocusEvent::Error { message: format!("Backend '{name}' is not available") };
            }
        }
    }

    #[plexus_macros::hub_method(description = "Dump the current layout definition")]
    async fn layout(&self) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.dump_layout().await {
                Ok(content) => yield LocusEvent::Layout { content },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }
}

#[async_trait]
impl ChildRouter for InfoActivation {
    fn router_namespace(&self) -> &'static str {
        "info"
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
