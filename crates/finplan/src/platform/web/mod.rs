//! Web platform implementations using browser APIs.

mod storage;
mod worker;

pub use storage::WebStorage;
pub use worker::WebWorker;
