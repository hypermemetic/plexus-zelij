//! Plexus activation implementations
//!
//! Each module provides a Plexus activation that exposes RPC methods for a specific
//! domain of terminal orchestration functionality.

/// System information and status activation
pub mod info;
/// Terminal state observation activation
pub mod observation;
/// Pane management activation
pub mod panes;
/// Terminal recording activation
pub mod recording;
/// Multi-pane rendering/compositing activation
pub mod render;
/// Session management activation
pub mod sessions;
/// Tab/window management activation
pub mod tabs;
/// Workspace configuration activation
pub mod workspace;

pub use info::InfoActivation;
pub use observation::ObservationActivation;
pub use panes::PanesActivation;
pub use recording::RecordingActivation;
pub use render::RenderActivation;
pub use sessions::SessionsActivation;
pub use tabs::TabsActivation;
pub use workspace::WorkspaceActivation;
