use std::sync::Arc;

use crate::activations::{
    InfoActivation, PanesActivation, RecordingActivation, RenderActivation, SessionsActivation,
    TabsActivation, WorkspaceActivation,
};
use crate::backend::TerminalBackend;

/// Locus — terminal workspace orchestration.
///
/// This is the factory that creates all sub-activations sharing the same backend.
/// Sub-activations are registered directly with the `DynamicHub` for flat routing:
///   synapse locus sessions list
///   synapse locus tabs list
///   synapse locus panes capture --pane %5
///   synapse locus workspace up --workspace dev
///   synapse locus info status
///   synapse locus recording start
///   synapse locus render render
pub struct Locus {
    pub sessions: SessionsActivation,
    pub tabs: TabsActivation,
    pub panes: PanesActivation,
    pub workspace: WorkspaceActivation,
    pub info: InfoActivation,
    pub recording: RecordingActivation,
    pub render: RenderActivation,
}

impl Locus {
    pub fn new(backend: impl TerminalBackend) -> Self {
        let backend: Arc<dyn TerminalBackend> = Arc::new(backend);
        Self {
            sessions: SessionsActivation::new(backend.clone()),
            tabs: TabsActivation::new(backend.clone()),
            panes: PanesActivation::new(backend.clone()),
            workspace: WorkspaceActivation::new(backend.clone()),
            info: InfoActivation::new(backend.clone()),
            recording: RecordingActivation::new(backend.clone()),
            render: RenderActivation::new(),
        }
    }
}
