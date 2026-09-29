fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // A sibling of `--dump-openapi`, and the only reason the committed spec is
    // read at all: the route-set gate runs when asked for, never at boot.
    if let Some(index) = args.iter().position(|arg| arg == "--check-openapi") {
        let spec = args
            .get(index + 1)
            .filter(|next| !next.starts_with('-'))
            .map_or_else(picasu::openapi_parity::default_spec_path, Into::into);
        std::process::exit(picasu::openapi_parity::run(&spec));
    }

    if args.iter().any(|arg| arg == "--dump-openapi") {
        print!("{}", picasu::openapi_public::public_json());
        return;
    }

    picasu::run();
}
