use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use std::sync::Arc;

use crate::backend::TerminalBackend;
use crate::plexus::{ChildRouter, PlexusError, PlexusStream, Activation};
use crate::types::*;

/// Tabs sub-activation — manages terminal tabs (tmux windows).
///
/// Accessed as `locus.tabs.list`, `locus.tabs.create`, etc.
#[derive(Clone)]
pub struct TabsActivation {
    pub(crate) backend: Arc<dyn TerminalBackend>,
}

impl TabsActivation {
    pub fn new(backend: Arc<dyn TerminalBackend>) -> Self {
        Self { backend }
    }
}

#[plexus_macros::hub_methods(
    namespace = "tabs",
    version = "0.1.0",
    description = "Terminal tab management"
)]
impl TabsActivation {
    #[plexus_macros::hub_method(
        description = "List tabs in a session",
        params(session = "Target session (default: current)")
    )]
    async fn list(
        &self,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.list_tabs(session.as_deref()).await {
                Ok(tabs) => yield LocusEvent::Tabs { tabs },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Create a new tab",
        params(
            name = "Tab name",
            layout = "Layout file path",
            cwd = "Working directory",
            session = "Target session (default: current)"
        )
    )]
    async fn create(
        &self,
        name: Option<String>,
        layout: Option<String>,
        cwd: Option<String>,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let opts = TabOpts {
                name,
                layout,
                cwd: cwd.map(Into::into),
                session,
            };
            match backend.create_tab(&opts).await {
                Ok(tab) => {
                    // Find the initial pane in the new tab
                    let initial_pane = backend.list_panes(None, None).await
                        .ok()
                        .and_then(|panes| {
                            panes.iter()
                                .find(|p| p.tab == tab.id)
                                .map(|p| p.id.clone())
                        });
                    yield LocusEvent::TabCreated { tab, initial_pane };
                }
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Close a tab by index",
        params(
            index = "Tab index",
            session = "Target session (default: current)"
        )
    )]
    async fn close(
        &self,
        index: u32,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.close_tab(session.as_deref(), index).await {
                Ok(()) => yield LocusEvent::Ok { message: format!("Closed tab {}", index) },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Focus a tab by index",
        params(
            index = "Tab index",
            session = "Target session (default: current)"
        )
    )]
    async fn focus(
        &self,
        index: u32,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.focus_tab(session.as_deref(), index).await {
                Ok(()) => yield LocusEvent::Ok { message: format!("Focused tab {}", index) },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Rename a tab",
        params(
            index = "Tab index",
            name = "New tab name",
            session = "Target session (default: current)"
        )
    )]
    async fn rename(
        &self,
        index: u32,
        name: String,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.rename_tab(session.as_deref(), index, &name).await {
                Ok(()) => yield LocusEvent::Ok { message: format!("Renamed tab {} to {}", index, name) },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }
}

#[async_trait]
impl ChildRouter for TabsActivation {
    fn router_namespace(&self) -> &str {
        "tabs"
    }

    async fn router_call(&self, method: &str, params: serde_json::Value) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, _name: &str) -> Option<Box<dyn ChildRouter>> {
        None
    }
}
