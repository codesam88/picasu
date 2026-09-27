// Drift fixture for the auth policy: the route table of a tree whose operations
// are all listed, one of which is behind a guard its handler does not declare.
pub mod data;
pub mod page;

pub fn generate_get_routes() -> Vec<Route> {
    routes![
        data::get_data,
        data::get_rows,
        data::get_scroll_bar,
        data::get_tags,
        page::login,
        page::setting,
        page::trashed,
    ]
}
