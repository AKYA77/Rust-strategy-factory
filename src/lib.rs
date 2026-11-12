pub mod candles;
pub mod contracts;
pub mod data;
pub mod indicators;
pub mod strategies;
pub mod tick_engine;

pub type Result<T> = std::result::Result<T, String>;
pub mod orders;
pub mod storage;
pub mod tick_index;
pub mod validation;
