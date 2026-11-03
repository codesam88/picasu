use anyhow::anyhow;
use log::{debug, error, warn};

use crate::{model::abstract_data::AbstractData, process::index::process_media_info};

/// The outcome of the index stage: the processed record plus the pipeline
/// result. The caller flushes `data` through the serial writer in both cases
/// — on failure the record is an error-marked placeholder (`pending`) that
/// must still be visible — and then returns `result` to the caller.
pub(crate) struct IndexOutcome {
    pub data: AbstractData,
    pub result: anyhow::Result<()>,
}

pub(crate) fn index_task_match(abstract_data: AbstractData) -> IndexOutcome {
    let hash = abstract_data.hash();
    let outcome = index_task(abstract_data);
    match outcome.result {
        Ok(()) => debug!("indexed {hash}"),
        Err(ref e) => error!("indexing failed {hash}: {e:#}"),
    }
    outcome
}

fn index_task(mut abstract_data: AbstractData) -> IndexOutcome {
    let hash = abstract_data.hash();
    // Path-primary records hold a single file entry.
    let Some(path) = abstract_data.path().map(|entry| entry.file.clone()) else {
        return IndexOutcome {
            data: abstract_data,
            result: Err(anyhow!("record has no path for hash: {hash}")),
        };
    };

    if !matches!(abstract_data.ext_type(), "image" | "video") {
        let ext = abstract_data.ext_type().to_string();
        return IndexOutcome {
            data: abstract_data,
            result: Err(anyhow!("unsupported file type: {ext}")),
        };
    }

    debug!("indexing {} {hash}: {path}", abstract_data.ext_type());

    let is_image = abstract_data.is_image();
    // One shared pipeline for both orchestration modes: the incremental index
    // here, the filesystem rebuild in `crate::process::rebuild`.
    if let Err(e) = process_media_info(&mut abstract_data) {
        debug!(
            "Failed {} data dump: {abstract_data:#?}",
            abstract_data.ext_type()
        );
        // Keep the file visible as an error-marked placeholder instead of
        // dropping it: identity + a `pending` payload are flushed through the
        // normal store path so the row remains album-filterable and
        // downloadable with a tooltip.
        warn!(
            "indexing failed for {} (hash {}, path {}): {e:#}",
            abstract_data.ext_type(),
            abstract_data.hash(),
            path
        );
        abstract_data.set_pending(true);
        let err = anyhow!(
            "failed to process {} metadata pipeline. Hash: {}, Path: {}: {e:#}",
            abstract_data.ext_type(),
            abstract_data.hash(),
            path
        );
        return IndexOutcome {
            data: abstract_data,
            result: Err(err),
        };
    }
    if !is_image {
        abstract_data.set_pending(true);
    }

    IndexOutcome {
        data: abstract_data,
        result: Ok(()),
    }
}
