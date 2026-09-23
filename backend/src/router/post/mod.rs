// src/router/post/mod.rs
use rocket::Route;
pub mod album_index;
pub mod authenticate;
pub mod create_dir_album;
pub mod create_share;
pub mod import_config;
pub mod post_upload;
pub mod rebuild;

pub fn generate_post_routes() -> Vec<Route> {
    routes![
        authenticate::authenticate,
        post_upload::upload,
        create_share::create_share,
        create_dir_album::create_dir_album,
        import_config::import_config_handler,
        album_index::index_album_handler,
        album_index::index_image_handler,
        album_index::cancel_album_index_handler,
        rebuild::rebuild_handler,
    ]
}
