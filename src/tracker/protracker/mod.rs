//! ProTracker 2 (.MOD) engine: a port of the replayer, Paula emulation and filters of
//! 8bitbubsy's pt2-clone. Output is bit-identical to pt2-clone.
//!
//! Derived from pt2-clone, Copyright (c) Olav Sørensen — BSD 3-Clause, see LICENSE-BSD.

pub mod consts;
pub mod dsp;
pub mod engine;
pub mod module;
pub mod paula;
pub mod replayer;
pub mod tables;

pub use consts::{AmigaModel, TimingMode};
pub use engine::{Engine, EngineConfig};
pub use module::{LoadError, Module};
