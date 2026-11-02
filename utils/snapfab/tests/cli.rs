use std::path::PathBuf;
use std::process::Command;

fn snapfab() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_snapfab"))
}

#[test]
fn quiet_library_run_prints_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = Command::new(snapfab())
        .args([
            "library",
            "--dir",
            dir.path().to_str().expect("dir"),
            "--count",
            "2",
        ])
        .output()
        .expect("run snapfab");
    assert!(output.status.success(), "quiet run failed: {output:?}");
    assert!(
        output.stdout.is_empty(),
        "quiet stdout was: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "quiet stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn verbose_library_run_prints_progress() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = Command::new(snapfab())
        .args([
            "library",
            "--dir",
            dir.path().to_str().expect("dir"),
            "--count",
            "2",
            "--verbose",
        ])
        .output()
        .expect("run snapfab");
    assert!(output.status.success(), "verbose run failed: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Generated 2 images in"),
        "verbose stderr missing summary: {stderr}"
    );
}

#[test]
fn generators_help_lists_names_and_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = Command::new(snapfab())
        .args([
            "library",
            "--dir",
            dir.path().to_str().expect("dir"),
            "--count",
            "1",
            "--generators",
            "help",
        ])
        .output()
        .expect("run snapfab");
    assert!(output.status.success(), "help run failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for name in [
        "circles",
        "landscape",
        "wavy",
        "mountains",
        "mandelbrot",
        "julia",
        "geometric",
        "still_life",
    ] {
        assert!(
            stdout.contains(name),
            "help output missing {name}: {stdout}"
        );
    }
    assert!(
        stdout.contains("default"),
        "help output missing default marker: {stdout}"
    );
    assert!(
        output.stderr.is_empty(),
        "help should not print to stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn help_output_contains_fancy_only_marker() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = Command::new(snapfab())
        .args([
            "library",
            "--dir",
            dir.path().to_str().expect("dir"),
            "--count",
            "1",
            "--generators",
            "list",
        ])
        .output()
        .expect("run snapfab");
    assert!(output.status.success(), "list run failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("(fancy-only)"),
        "help output missing fancy-only marker: {stdout}"
    );
}

#[test]
fn fancy_plus_generators_conflicts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = Command::new(snapfab())
        .args([
            "library",
            "--dir",
            dir.path().to_str().expect("dir"),
            "--count",
            "1",
            "--fancy",
            "--generators",
            "circles",
        ])
        .output()
        .expect("run snapfab");
    assert!(!output.status.success(), "conflicting flags should fail");
}

#[test]
fn batch_generators_list_without_manifest() {
    let output = Command::new(snapfab())
        .args(["batch", "--generators", "list"])
        .output()
        .expect("run snapfab");
    assert!(output.status.success(), "list run failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("circles"), "stdout missing list: {stdout}");
}

#[test]
fn batch_generators_bogus_errors_without_creating_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest_path = dir.path().join("manifest.json");
    let out_path = dir.path().join("out.jpg");
    let manifest = format!(
        r#"[{{"output":"{}","format":"jpeg","width":4,"height":4,"minimal":true}}]"#,
        out_path.display()
    );
    std::fs::write(&manifest_path, manifest).expect("write manifest");
    let output = Command::new(snapfab())
        .args([
            "batch",
            manifest_path.to_str().expect("manifest"),
            "--generators",
            "bogus",
        ])
        .output()
        .expect("run snapfab");
    assert!(!output.status.success(), "bogus generator should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown generator"),
        "stderr missing unknown generator: {stderr}"
    );
    assert!(
        !out_path.exists(),
        "output file was created despite validation failure"
    );
}

#[test]
fn batch_quiet_with_stdin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut entries = Vec::new();
    for i in 0..10 {
        let path = dir.path().join(format!("img_{i}.jpg"));
        entries.push(format!(
            r#"{{"output":"{}","format":"jpeg","width":4,"height":4,"minimal":true}}"#,
            path.display()
        ));
    }
    let manifest = format!("[{}]", entries.join(","));
    let output = Command::new(snapfab())
        .arg("batch")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(manifest.as_bytes())?;
            child.wait_with_output()
        })
        .expect("run snapfab");
    assert!(output.status.success(), "batch run failed: {output:?}");
    assert!(
        output.stdout.is_empty(),
        "quiet stdout was: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "quiet stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for i in 0..10 {
        assert!(dir.path().join(format!("img_{i}.jpg")).exists());
    }
}

#[test]
fn batch_verbose_with_stdin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut entries = Vec::new();
    for i in 0..10 {
        let path = dir.path().join(format!("img_{i}.jpg"));
        entries.push(format!(
            r#"{{"output":"{}","format":"jpeg","width":4,"height":4,"minimal":true}}"#,
            path.display()
        ));
    }
    let manifest = format!("[{}]", entries.join(","));
    let output = Command::new(snapfab())
        .args(["batch", "-", "--verbose"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(manifest.as_bytes())?;
            child.wait_with_output()
        })
        .expect("run snapfab");
    assert!(output.status.success(), "verbose batch failed: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Generated 10 images"),
        "verbose stderr missing summary: {stderr}"
    );
}
