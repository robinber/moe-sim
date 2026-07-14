//! Core domain types and validation for moe-sim.
//!
//! This crate defines the canonical event format and its invariants.
//! All higher-level simulation logic must be built on top of these contracts.

pub mod trace;

/// Re-exports the primary public API for the canonical trace events.
pub use trace::{Event, EventError, Phase};
