use crate::public::constant::storage::get_data_path;
use log::info;

pub fn initialize_folder() {
    let root = get_data_path();
    info!("Storage root initialized at: {}", root.display());
    std::fs::create_dir_all(root.join("db")).unwrap();

    std::fs::create_dir_all(root.join("object/compressed")).unwrap();

    // Pre-create the default UROCISSA_IMAGE_HOME (see operations::utils::image_path)
    // so it's discoverable with zero configuration, even before `imagePath` is set.
    // Skipped if the env var points elsewhere — nothing to create here in that case.
    if std::env::var("UROCISSA_IMAGE_HOME").is_err() {
        std::fs::create_dir_all(root.join("images")).unwrap();
    }
}
