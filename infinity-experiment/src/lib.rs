//! Executable Software Factory prototype built on Infinity Runtime.
//!
//! This crate is deliberately isolated from the production Factory execution
//! path. It exists to answer one question with running code: should Factory
//! replace its custom agent execution/orchestration layer with Infinity?

pub mod effects;
pub mod engineering;
pub mod journal;
pub mod model;
pub mod rap;
pub mod runtime;
pub mod store;
pub mod subscription;
