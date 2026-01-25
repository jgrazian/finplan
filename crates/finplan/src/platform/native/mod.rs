//! Native platform implementations using filesystem and threads.

mod storage;
mod worker;

pub use storage::NativeStorage;
pub use worker::NativeWorker;
