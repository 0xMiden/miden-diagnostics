use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn run_cargo(manifest: &Path, target_dir: &Path, arguments: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO"));
    command
        .arg("--color=never")
        .args(arguments)
        .arg("--manifest-path")
        .arg(manifest)
        .args(["--locked"])
        .env("CARGO_TARGET_DIR", target_dir);
    command.output().expect("fixture cargo command must launch")
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn assert_failure(output: &Output, context: &str, expected: &[&str]) {
    assert!(
        !output.status.success(),
        "{context} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for message in expected {
        assert!(
            stderr.contains(message),
            "{context} did not contain {message:?}\nstderr:\n{stderr}"
        );
    }
}

#[test]
fn derive_external_feature_and_ui_contracts_are_executable() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_dir: PathBuf = crate_root.join("../../target/derive-compile");
    let fixtures = crate_root.join("tests/fixtures");

    let explanations = fixtures.join("derive-explanations/Cargo.toml");
    let disabled = run_cargo(
        &explanations,
        &target_dir,
        &["test", "-p", "derive-definitions", "--test", "disabled"],
    );
    assert_success(&disabled, "disabled explanation fixture");

    let enabled =
        run_cargo(&explanations, &target_dir, &["run", "-p", "derive-explanation-app", "--quiet"]);
    assert_success(&enabled, "enabled explanation fixture");
    assert!(String::from_utf8_lossy(&enabled.stdout).contains("embedded=true; not_provided=true"));

    let discard =
        run_cargo(&fixtures.join("derive-discard/Cargo.toml"), &target_dir, &["test", "--quiet"]);
    assert_success(&discard, "discarded nonexistent explanation fixture");

    let ui = fixtures.join("derive-ui/Cargo.toml");
    let portable =
        run_cargo(&ui, &target_dir, &["check", "--lib", "--target", "wasm32-unknown-unknown"]);
    assert_success(&portable, "renamed derive-only no-std fixture");

    for (binary, messages) in [
        (
            "reject-attributes",
            &["duplicate diagnostic `message` argument", "unsupported diagnostic argument"][..],
        ),
        (
            "reject-primary-shape",
            &[
                "at most one statically declared primary label is allowed",
                "a primary label field cannot be a collection",
                "a diagnostic source cannot be a collection",
            ][..],
        ),
        (
            "reject-warning-report",
            &["report! cannot construct a literal Warning diagnostic"][..],
        ),
        (
            "reject-info-report",
            &["report! cannot construct a literal Info diagnostic"][..],
        ),
        (
            "reject-hint-report",
            &["report! cannot construct a literal Hint diagnostic"][..],
        ),
        ("reject-code", &["diagnostic code must be `namespace/code`"][..]),
        ("reject-declaration-code", &["diagnostic namespace must not be empty"][..]),
    ] {
        let output =
            run_cargo(&ui, &target_dir, &["check", "--bin", binary, "--message-format=short"]);
        assert_failure(&output, binary, messages);
    }
}
