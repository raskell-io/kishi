//! Kishi's Go engine.

pub mod board;
pub mod mcts;
pub mod patterns;

pub use board::Board;
pub use mcts::{Limits, Searcher};

#[cfg(feature = "web")]
pub mod web;
