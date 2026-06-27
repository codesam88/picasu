fn main() {
    if std::env::args().any(|a| a == "--dump-openapi") {
        print!("{}", picasu::openapi::generate_json());
        return;
    }
    picasu::run();
}
