use crate::{error::handle_error, process::hash::blake3_hasher};
use anyhow::Result;
use arrayvec::ArrayString;
use mini_executor::Task;
use std::fs::File;

pub struct HashTask {
    pub file: File,
}

impl HashTask {
    pub fn new(file: File) -> Self {
        Self { file }
    }
}

impl Task for HashTask {
    type Output = Result<ArrayString<64>>;

    async fn run(self) -> Self::Output {
        tokio_rayon::spawn(move || hash_task(self.file))
            .await
            .map_err(|err| handle_error(err.context("Failed to run hash task")))
    }
}
fn hash_task(file: File) -> Result<ArrayString<64>> {
    blake3_hasher(file)
}
