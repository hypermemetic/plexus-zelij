pub mod backend;
pub mod backends;
pub mod types;
pub mod activation;
pub mod activations;
pub mod cast;
pub mod recording;
pub mod compositor;
pub mod observation;

// Re-exports required by plexus_macros generated code.
// The hub_methods macro references crate::plexus::* and crate::serde_helpers.
pub mod plexus {
    pub use plexus_core::plexus::*;
    pub use plexus_core::types::Handle;
}
pub use plexus_core::serde_helpers;

// Public API
pub use activation::Locus;
pub use backend::TerminalBackend;
pub use backends::{Zellij, TmuxBackend};
