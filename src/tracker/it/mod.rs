//! Impulse Tracker (.IT) and Scream Tracker 3 (.S3M) engine.
//!
//! Uses formulas and tables from OpenMPT — BSD 3-Clause, see LICENSE-BSD.

pub mod format;
pub mod s3m;
pub mod player;

pub use format::ItModule;
pub use player::ItPlayer;
