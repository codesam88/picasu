fn main() {
    if std::env::args().any(|a| a == "--dump-openapi") {
        print!("{}", picasu::openapi_public::public_json());
        return;
    }
    picasu::run();
}
