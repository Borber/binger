//! Core logic for binger.
//!
//! The selection state machine is intentionally independent of Wayland. This
//! keeps the geometry and keyboard behavior testable without a running
//! compositor and leaves the Wayland layer surface as an adapter around it.

pub mod bisect;
pub mod config;
pub mod input;
#[cfg(feature = "wayland-backend")]
pub mod wayland;
