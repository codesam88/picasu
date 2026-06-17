mod generator;

use std::process::Command;

fn main() {
    let subcommand = std::env::args().nth(1);

    match subcommand.as_deref() {
        Some("emit-openapi") => emit_openapi(),
        Some("gen-scenarios") => generator::generate_all(),
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            eprintln!("available: emit-openapi, gen-scenarios");
            std::process::exit(1);
        }
        None => {
            eprintln!("missing subcommand");
            eprintln!("available: emit-openapi, gen-scenarios");
            std::process::exit(1);
        }
    }
}

fn emit_openapi() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "urocissa",
            "--bin",
            "urocissa-openapi",
            "--features",
            "openapi",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .output()
        .expect("failed to run urocissa-openapi");

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    let spec = String::from_utf8(output.stdout).expect("openapi output is not valid UTF-8");
    let path = std::path::Path::new("gallery-backend").join("openapi.json");
    std::fs::write(&path, spec.as_bytes()).expect("failed to write openapi.json");

    eprintln!("wrote {}", path.display());
}
