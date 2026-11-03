use crate::tasks::runtime::RUNTIME;
use mini_executor::TaskExecutor;
use std::sync::LazyLock;

pub mod actor;
pub mod batcher;
pub mod looper;
pub mod queue;
pub mod runtime;

pub static INDEX_COORDINATOR: LazyLock<TaskExecutor> =
    LazyLock::new(|| TaskExecutor::new(&RUNTIME));

pub static BATCH_COORDINATOR: LazyLock<TaskExecutor> =
    LazyLock::new(|| TaskExecutor::new(&RUNTIME));
