//! Core types for moe-sim (canonical events, model sizes, and validation).
//!
//! Expert sizes are always explicit: see [`ModelManifest`]. Simulators and
//! policies must not invent missing sizes.

pub mod manifest;
pub mod trace;

pub use manifest::{CapacityError, ExpertKey, ExpertSizeEntry, ManifestError, ModelManifest};
pub use trace::{Event, EventError, EventParts, Phase};
