//! Kishi: a chess engine for the browser.

#![cfg_attr(feature = "threads", feature(stdarch_wasm_atomic_wait))]

pub mod attacks;
pub mod hce;
pub mod movegen;
pub mod nnue;
pub mod position;
pub mod search;
pub mod tt;
pub mod types;

pub use position::{Position, START_FEN};
pub use search::{Limits, SearchResult, Searcher};
pub use types::Move;

#[cfg(feature = "web")]
pub mod web;
