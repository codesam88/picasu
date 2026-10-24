// src/router/post/mod.rs
use rocket::Route;
pub mod album_index;
pub mod config_auth_key;
pub mod create_dir_album;
pub mod create_share;
pub mod import_config;
pub mod invites;
pub mod post_upload;
pub mod users_authenticate;
pub mod users_bootstrap;
pub mod users_register;

pub fn generate_post_routes() -> Vec<Route> {
    routes![
        users_authenticate::authenticate,
        users_authenticate::logout,
        post_upload::upload,
        create_share::create_share,
        create_dir_album::create_dir_album,
        import_config::import_config_handler,
        album_index::index_album_handler,
        album_index::cancel_album_index_handler,
        users_bootstrap::bootstrap_admin_handler,
        users_register::register_user_handler,
        config_auth_key::reset_auth_key_handler,
        invites::create_invite_handler,
    ]
}
