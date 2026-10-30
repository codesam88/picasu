// Fixed-tree fixture: no annotations at all. The walk counts the file and
// ignores its items, so a file the scan cannot learn anything from still
// proves the walk reached it.

/// Not a handler: no utoipa annotation, no route.
pub fn helper(value: String) -> String {
    value
}
