pub mod candles;
pub mod contracts;
pub mod data;
pub mod indicators;
pub mod strategies;
pub mod tick_engine;

pub type Result<T> = std::result::Result<T, String>;
pub mod orders;
pub mod risk;
pub mod storage;
pub mod tick_index;
pub mod validation;

pub const ENGINE_VERSION: &str = "ordered-l1-v2-marked-margin";
pub const INDICATOR_VERSION: &str = "causal-primitives-v2-rsi-bps";
pub const SIGNAL_VERSION: &str = "closed-publication-next-event-v1";
pub mod dashboard;
pub mod research;
pub mod screen;
