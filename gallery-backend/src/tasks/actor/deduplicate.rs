use crate::{
    operations::open_db::open_data_table,
    public::{error_data::handle_error, structure::abstract_data::AbstractData},
    tasks::{BATCH_COORDINATOR, batcher::flush_tree::FlushTreeTask},
};
use anyhow::Result;
use arrayvec::ArrayString;
use mini_executor::Task;
use std::{mem, path::PathBuf};
use tokio::task::spawn_blocking;

pub struct DeduplicateTask {
    pub path: PathBuf,
    pub hash: ArrayString<64>,
    pub presigned_album_id: Option<ArrayString<64>>,
}

impl DeduplicateTask {
    pub fn new(
        path: PathBuf,
        hash: ArrayString<64>,
        presigned_album_id: Option<ArrayString<64>>,
    ) -> Self {
        Self {
            path,
            hash,
            presigned_album_id,
        }
    }
}

impl Task for DeduplicateTask {
    type Output = Result<Option<AbstractData>>;

    async fn run(self) -> Self::Output {
        spawn_blocking(move || deduplicate_task(&self))
            .await
            .expect("blocking task panicked")
            .map_err(|err| handle_error(err.context("Failed to run deduplicate task")))
    }
}

fn deduplicate_task(task: &DeduplicateTask) -> Result<Option<AbstractData>> {
    let mut abstract_data = AbstractData::new(&task.path, task.hash)?;

    let data_table = open_data_table();

    if let Some(guard) = data_table.get(&*task.hash).unwrap() {
        let mut data_exist = guard.value();
        if let Some(alias_mut) = abstract_data.alias_mut() {
            let file_modify = mem::take(&mut alias_mut[0]);
            if let Some(exist_alias) = data_exist.alias_mut() {
                exist_alias.push(file_modify);
            }
        }
        if let Some(album_id) = task.presigned_album_id {
            data_exist.set_album(Some(album_id));
        }
        BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::insert(vec![data_exist]));
        warn!("File already exists in the database:\n{:#?}", abstract_data);
        Ok(None)
    } else {
        if let Some(album_id) = task.presigned_album_id {
            abstract_data.set_album(Some(album_id));
        }
        Ok(Some(abstract_data))
    }
}
