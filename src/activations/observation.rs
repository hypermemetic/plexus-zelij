//! Terminal observation activation - efficient state queries for agents
//!
//! Provides in-memory terminal state access without file I/O.
//! Integrates with the recording engine's `TerminalStateManager`.

use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use std::sync::Arc;
use std::time::Duration;

use crate::backend::TerminalBackend;
use crate::observation::TerminalStateManager;
use crate::plexus::{Activation, ChildRouter, PlexusError, PlexusStream};
use crate::types::{LocusEvent, PaneId};

/// Observation activation - provides efficient terminal state queries
#[derive(Clone)]
pub struct ObservationActivation {
    /// Terminal backend instance (currently unused but kept for future use)
    _backend: Arc<dyn TerminalBackend>,
    /// Shared terminal state manager for in-memory VT100 state tracking
    terminal_state: Arc<TerminalStateManager>,
}

impl ObservationActivation {
    /// Create a new ObservationActivation with backend and terminal state manager
    pub fn new(
        backend: Arc<dyn TerminalBackend>,
        terminal_state: Arc<TerminalStateManager>,
    ) -> Self {
        Self { _backend: backend, terminal_state }
    }
}

#[allow(missing_docs)]
#[plexus_macros::hub_methods(
    namespace = "observation",
    version = "0.1.0",
    description = "Efficient terminal state observation with zero file I/O"
)]
impl ObservationActivation {
    #[plexus_macros::hub_method(
        description = "Get current screen contents from in-memory state (instant, no file I/O)",
        params(pane = "Pane ID (e.g. '%5')")
    )]
    async fn get_screen(&self, pane: String) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let terminal_state = self.terminal_state.clone();
        stream! {
            match terminal_state.get_contents(&pane).await {
                Some(contents) => {
                    yield LocusEvent::ScreenContent {
                        pane: PaneId(pane),
                        contents,
                    };
                }
                None => {
                    yield LocusEvent::Error {
                        message: format!("Pane {pane} not being tracked"),
                    };
                }
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Get cursor position (row, col) - 0-indexed",
        params(pane = "Pane ID (e.g. '%5')")
    )]
    async fn get_cursor(&self, pane: String) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let terminal_state = self.terminal_state.clone();
        stream! {
            match terminal_state.get_cursor(&pane).await {
                Some((row, col)) => {
                    yield LocusEvent::CursorPosition {
                        pane: PaneId(pane),
                        row,
                        col,
                    };
                }
                None => {
                    yield LocusEvent::Error {
                        message: format!("Pane {pane} not being tracked"),
                    };
                }
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Get specific region of terminal (e.g., last N lines)",
        params(
            pane = "Pane ID (e.g. '%5')",
            start_row = "Start row (0-indexed)",
            end_row = "End row (exclusive)"
        )
    )]
    async fn get_region(
        &self,
        pane: String,
        start_row: u16,
        end_row: u16,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let terminal_state = self.terminal_state.clone();
        stream! {
            match terminal_state.get_region(&pane, start_row, end_row).await {
                Some(content) => {
                    let text = String::from_utf8_lossy(&content).to_string();
                    yield LocusEvent::RegionContent {
                        pane: PaneId(pane),
                        content: text,
                    };
                }
                None => {
                    yield LocusEvent::Error {
                        message: format!("Pane {pane} not being tracked"),
                    };
                }
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Wait for terminal content to change (event-driven, no polling!)",
        params(
            pane = "Pane ID (e.g. '%5')",
            timeout_ms = "Max time to wait in milliseconds (default: 5000)"
        )
    )]
    async fn wait_for_change(
        &self,
        pane: String,
        timeout_ms: Option<u64>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let terminal_state = self.terminal_state.clone();
        stream! {
            let timeout = Duration::from_millis(timeout_ms.unwrap_or(5000));

            // Subscribe to changes
            let mut rx = if let Some(rx) = terminal_state.subscribe(&pane).await { rx } else {
                yield LocusEvent::Error {
                    message: format!("Pane {pane} not being tracked"),
                };
                return;
            };

            // Wait for next change or timeout
            match tokio::time::timeout(timeout, rx.changed()).await {
                Ok(Ok(())) => {
                    // Got a change! Fetch current contents
                    if let Some(contents) = terminal_state.get_contents(&pane).await {
                        let sequence = terminal_state.get_sequence(&pane).await.unwrap_or(0);
                        yield LocusEvent::ScreenChanged {
                            pane: PaneId(pane),
                            contents,
                            sequence,
                        };
                    }
                }
                Ok(Err(_)) => {
                    yield LocusEvent::Error {
                        message: "Change notification channel closed".to_string(),
                    };
                }
                Err(_) => {
                    yield LocusEvent::Timeout;
                }
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Get changes since a specific sequence number (incremental fetching)",
        params(pane = "Pane ID (e.g. '%5')", sequence = "Last known sequence number")
    )]
    async fn get_changes_since(
        &self,
        pane: String,
        sequence: u64,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let terminal_state = self.terminal_state.clone();
        stream! {
            let current_seq = if let Some(seq) = terminal_state.get_sequence(&pane).await { seq } else {
                yield LocusEvent::Error {
                    message: format!("Pane {pane} not being tracked"),
                };
                return;
            };

            if current_seq > sequence {
                // Content has changed, return it
                if let Some(contents) = terminal_state.get_contents(&pane).await {
                    yield LocusEvent::ScreenContent {
                        pane: PaneId(pane.clone()),
                        contents,
                    };
                    yield LocusEvent::SequenceUpdate {
                        pane: PaneId(pane),
                        sequence: current_seq,
                    };
                }
            } else {
                // No changes
                yield LocusEvent::NoChanges {
                    sequence: current_seq,
                };
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Get terminal dimensions (width, height)",
        params(pane = "Pane ID (e.g. '%5')")
    )]
    async fn get_dimensions(
        &self,
        pane: String,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let terminal_state = self.terminal_state.clone();
        stream! {
            match terminal_state.get_dimensions(&pane).await {
                Some((width, height)) => {
                    yield LocusEvent::Dimensions {
                        pane: PaneId(pane),
                        width,
                        height,
                    };
                }
                None => {
                    yield LocusEvent::Error {
                        message: format!("Pane {pane} not being tracked"),
                    };
                }
            }
        }
    }

    #[plexus_macros::hub_method(description = "List all panes being tracked in terminal state")]
    async fn list_tracked(&self) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let terminal_state = self.terminal_state.clone();
        stream! {
            let tracked = terminal_state.tracked_panes().await;
            yield LocusEvent::TrackedPanes { panes: tracked };
        }
    }

    #[plexus_macros::hub_method(
        description = "Get info about all tracked panes (dimensions, sequence, last update)"
    )]
    async fn get_all_info(&self) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let terminal_state = self.terminal_state.clone();
        stream! {
            let infos = terminal_state.get_all_info().await;
            yield LocusEvent::PaneStateInfos { infos };
        }
    }
}

#[async_trait]
impl ChildRouter for ObservationActivation {
    fn router_namespace(&self) -> &'static str {
        "observation"
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
