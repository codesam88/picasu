use crate::model::abstract_data::AbstractData;
use crate::model::asset::AssetRecord;
use crate::model::metadata_record::compose_abstract_data;
use crate::router::auth::GuardAuth;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::{ASSET_BY_ID, METADATA_TABLE, TREE};
use redb::{ReadableDatabase, ReadableTable};
use rocket::get;
use rocket::response::stream::ByteStream;
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct ExportEntry {
    key: String,
    value: AbstractData,
}

#[utoipa::path(
        get,
        path = "/get/get-export",
        responses(
            (status = 200, description = "Export data as JSON"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[get("/get/get-export")]
pub fn get_export(auth: GuardResult<GuardAuth>) -> AppResult<ByteStream![Vec<u8>]> {
    let _ = auth?;
    let Ok(read_txn) = TREE.in_disk.begin_read() else {
        return Err(crate::error::AppError::new(
            crate::error::ErrorKind::Database,
            "Failed to begin read transaction",
        ));
    };
    let Ok(metadata_table) = read_txn.open_table(METADATA_TABLE) else {
        return Err(crate::error::AppError::new(
            crate::error::ErrorKind::Database,
            "Failed to open METADATA_TABLE",
        ));
    };
    let Ok(id_table) = read_txn.open_table(ASSET_BY_ID) else {
        return Err(crate::error::AppError::new(
            crate::error::ErrorKind::Database,
            "Failed to open ASSET_BY_ID",
        ));
    };
    let byte_stream = ByteStream! {
        // Open DB and prepare to iterate
        let Ok(iter) = metadata_table.iter() else {
            yield b"{\"error\":\"failed to iterate\"}".to_vec();
            return;
        };

        // Start the JSON array
        yield b"[".to_vec();
        let mut first = true;

        for entry_res in iter {
            let Ok((key, value)) = entry_res else {
                // Skip or handle the error
                continue;
            };

            // Compose each payload with its identity record so the export
            // keeps the full AbstractData shape.
            let Ok(Some(record_json)) = id_table.get(key.value()) else {
                continue;
            };
            let Ok(record) = serde_json::from_str::<AssetRecord>(record_json.value()) else {
                continue;
            };

            // Insert a comma if not the first element
            if !first {
                yield b",".to_vec();
            }
            first = false;

            // Build the ExportEntry
            let export = ExportEntry {
                key: key.value().to_string(),
                value: compose_abstract_data(&record, Some(&value.value())),
            };

            // Convert it to JSON
            let Ok(json_obj) = serde_json::to_string(&export) else {
                // Skip or handle the error
                continue;
            };

            // Stream it out
            yield json_obj.into_bytes();
        }

        // End the JSON array
        yield b"]".to_vec();
    };
    Ok(byte_stream)
}
