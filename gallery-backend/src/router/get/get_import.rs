use rocket::get;
use rocket::serde::json::Json;

use crate::router::fairing::guard_auth::GuardAuth;
use crate::tasks::actor::folder_import::{FolderImportStatus, folder_import_status};

#[get("/get/import/folder/status")]
pub fn get_folder_import_status(_auth: GuardAuth) -> Json<FolderImportStatus> {
    Json(folder_import_status())
}
