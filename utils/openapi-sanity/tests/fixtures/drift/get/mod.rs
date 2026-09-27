// Drift fixture: the same route table as the clean fixture, with `page::login`
// registered twice.
pub mod data;
pub mod page;
pub mod probe;

pub fn generate_get_routes() -> Vec<Route> {
    routes![
        page::login,
        page::login,
        data::get_data,
        data::get_rows,
        data::path_completion,
        data::get_metadata,
        probe::probe_record,
    ]
}
