//! FastTracker II (.XM) engine: a port of ft2-clone's replayer.
//!
//! Derived from ft2-clone, Copyright (c) Olav Sørensen — BSD 3-Clause, see LICENSE-BSD.

pub mod format;
pub mod player;
pub mod tables;

pub use format::XmModule;
pub use player::XmPlayer;
