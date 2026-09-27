// Tag drift fixture: a tree whose annotations and document agree — the state
// after a regeneration — and whose tagging violates the taxonomy in one place per
// rule. The `clean/` tree is the baseline these expectations are measured against.
pub mod data;
pub mod page;

pub fn generate_get_routes() -> Vec<Route> {
    routes![
        data::get_data,
        data::edit_tag,
        data::get_albums,
        data::probe_record,
        page::login,
        page::setting,
    ]
}
