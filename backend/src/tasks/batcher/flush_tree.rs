use mini_executor::BatchTask;
use std::path::Path;

use crate::{
    model::abstract_data::AbstractData,
    process::dir_album::{mark_album_for_update, mark_dir_albums_for_path},
    storage::db::{DATA_TABLE, TREE},
    tasks::{BATCH_COORDINATOR, batcher::update_tree::UpdateTreeTask},
};

pub struct FlushTreeTask {
    pub insert_list: Vec<AbstractData>,
    pub remove_list: Vec<AbstractData>,
}

impl FlushTreeTask {
    pub fn insert(data_list: Vec<AbstractData>) -> Self {
        Self {
            insert_list: data_list,
            remove_list: Vec::new(),
        }
    }

    pub fn remove(abstract_data_list: Vec<AbstractData>) -> Self {
        Self {
            insert_list: Vec::new(),
            remove_list: abstract_data_list,
        }
    }
}

impl BatchTask for FlushTreeTask {
    async fn batch_run(list: Vec<Self>) {
        let mut all_insert_data = Vec::new();
        let mut all_remove_abstract_data = Vec::new();
        for task in list {
            all_insert_data.extend(task.insert_list);
            all_remove_abstract_data.extend(task.remove_list);
        }
        flush_tree_task(&all_insert_data, &all_remove_abstract_data);
    }
}

fn flush_tree_task(insert_list: &[AbstractData], remove_list: &[AbstractData]) {
    let write_txn = TREE
        .in_disk
        .begin_write()
        .expect("failed to begin write transaction");
    {
        let mut data_table = write_txn
            .open_table(DATA_TABLE)
            .expect("failed to open table");

        for abstract_data in insert_list {
            let hash = abstract_data.hash();
            data_table
                .insert(&*hash, abstract_data)
                .expect("failed to insert");
        }
        for abstract_data in remove_list {
            let hash = abstract_data.hash();
            data_table.remove(&*hash).expect("failed to remove");
        }
    };
    write_txn.commit().expect("failed to commit transaction");

    for abstract_data in insert_list.iter().chain(remove_list.iter()) {
        if let Some(album_id) = abstract_data.album() {
            mark_album_for_update(album_id);
        }
        for file_modify in abstract_data.alias() {
            mark_dir_albums_for_path(Path::new(&file_modify.file));
        }
    }

    BATCH_COORDINATOR.execute_batch_detached(UpdateTreeTask);
}
