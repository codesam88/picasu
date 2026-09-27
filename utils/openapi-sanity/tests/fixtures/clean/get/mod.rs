// Fixture route table. `openapi-sanity` reads this file for the `routes![]`
// entries and the other files of the tree for the handlers they name, the way
// the real router splits a route table from its handlers.
pub mod data;
pub mod page;
pub mod probe;

pub fn generate_get_routes() -> Vec<Route> {
    routes![
        page::login,
        data::get_data,
        data::get_rows,
        data::path_completion,
        data::get_metadata,
        probe::probe_record,
    ]
}
