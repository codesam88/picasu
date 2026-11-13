fn main() {
    println!("cargo:rerun-if-changed=pipeline/operations.json");
    println!("cargo:rerun-if-changed=pipeline/events.json");
}
