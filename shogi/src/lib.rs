//! Kishi's shogi engine.

pub mod eval;
pub mod position;
pub mod search;

pub use position::{Move, Position, START_SFEN};
pub use search::{Limits, SearchResult, Searcher};

#[cfg(feature = "web")]
pub mod web;
