use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use wasmi::{Engine, Linker, Module, Store};

const EXPLANATION_SENTINEL: &str = "LINKED_EXPLANATION_LOOKUP_ONLY_λ";
const SORTED_CODES: &str = "registry::alpha/E1000,registry::alpha/E2000,registry::alpha/I5000,\
                            registry::alpha/N4000,registry::beta/E1000,registry::beta/W3000";

fn fixture_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/registry/Cargo.toml")
}

fn target_dir(variant: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("registry-{variant}"))
}

fn cargo_build(package: &str, variant: &str, target: Option<&str>, features: &[&str]) -> PathBuf {
    let manifest = fixture_manifest();
    let target_dir = target_dir(variant);
    let mut command = Command::new(env!("CARGO"));
    command
        .args(["build", "--manifest-path"])
        .arg(&manifest)
        .args(["--release", "--locked", "--offline", "--no-default-features", "-p", package])
        .env("CARGO_TARGET_DIR", &target_dir);
    if let Some(target) = target {
        command.args(["--target", target]);
    }
    if !features.is_empty() {
        command.args(["--features", &features.join(",")]);
    }
    let output = command.output().expect("fixture cargo build must launch");
    assert_success(&output, &format!("{variant} fixture build"));
    target_dir
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn native_binary(target_dir: &Path) -> PathBuf {
    target_dir
        .join("release")
        .join(format!("registry-native-probe{}", std::env::consts::EXE_SUFFIX))
}

fn run(binary: &Path, arguments: &[&str]) -> Output {
    Command::new(binary)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to run {}: {error}", binary.display()))
}

#[test]
fn native_linked_registry_retention_explanations_and_governance_are_executable() {
    let unanchored_dir = cargo_build("registry-native-probe", "native-unanchored", None, &[]);
    let unanchored_binary = native_binary(&unanchored_dir);
    let unanchored = run(&unanchored_binary, &["linked"]);
    assert_success(&unanchored, "unanchored native control");
    assert_eq!(String::from_utf8_lossy(&unanchored.stdout).trim(), "linked=;count=0");
    let unanchored_bytes =
        fs::read(&unanchored_binary).expect("unanchored binary must be readable");
    assert!(
        !unanchored_bytes
            .windows(EXPLANATION_SENTINEL.len())
            .any(|window| window == EXPLANATION_SENTINEL.as_bytes()),
        "the unembedded release artifact must discard long explanation text"
    );

    let anchored_dir = cargo_build("registry-native-probe", "native-anchored", None, &["anchors"]);
    let anchored_binary = native_binary(&anchored_dir);
    let anchored = run(&anchored_binary, &["linked"]);
    assert_success(&anchored, "anchored native registry");
    let anchored_stdout = String::from_utf8_lossy(&anchored.stdout);
    assert!(anchored_stdout.contains(&format!("linked={SORTED_CODES};count=6")));
    assert!(anchored_stdout.contains("definition-only=true"));
    assert!(anchored_stdout.contains("occurrence-registration=false"));

    for mode in ["explain-explicit", "explain-linked"] {
        let output = run(&anchored_binary, &[mode, "registry::alpha/E1000"]);
        assert_eq!(output.status.code(), Some(3), "{mode} must report NotEmbedded");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stdout.contains("summary=alpha primary failure"));
        assert!(stdout.contains("url=https://example.com/registry/alpha/E1000"));
        assert!(stderr.contains("explanation=not-embedded"));
        assert!(!stdout.contains(EXPLANATION_SENTINEL));
        assert!(!stderr.contains(EXPLANATION_SENTINEL));
    }
    let ambiguous = run(&anchored_binary, &["explain-explicit", "E1000"]);
    assert_eq!(ambiguous.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&ambiguous.stderr)
            .contains("ambiguous=E1000;first=registry::alpha/E1000;second=registry::beta/E1000")
    );
    let unknown = run(&anchored_binary, &["explain-linked", "unknown"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown=unknown"));
    let not_provided = run(&anchored_binary, &["explain-explicit", "registry::alpha/N4000"]);
    assert_success(&not_provided, "known explanation-not-provided query");
    assert!(String::from_utf8_lossy(&not_provided.stdout).contains("explanation=not-provided"));

    let duplicate_dir =
        cargo_build("registry-native-probe", "native-duplicate", None, &["duplicate"]);
    let duplicate = run(&native_binary(&duplicate_dir), &["duplicate"]);
    assert_success(&duplicate, "duplicate linked registry");
    let duplicate_stdout = String::from_utf8_lossy(&duplicate.stdout);
    assert!(duplicate_stdout.contains("duplicate=registry::alpha/E1000"));
    assert!(duplicate_stdout.contains("registry_definitions_alpha"));
    assert!(duplicate_stdout.contains("registry_definitions_duplicate"));

    let embedded_dir =
        cargo_build("registry-native-probe", "native-embedded", None, &["anchors", "embed"]);
    let embedded_binary = native_binary(&embedded_dir);
    for mode in ["explain-explicit", "explain-linked"] {
        let output = run(&embedded_binary, &[mode, "registry::alpha/E1000"]);
        assert_success(&output, &format!("embedded {mode}"));
        assert!(String::from_utf8_lossy(&output.stdout).contains(EXPLANATION_SENTINEL));
    }
    let embedded_bytes = fs::read(&embedded_binary).expect("embedded binary must be readable");
    assert!(
        embedded_bytes
            .windows(EXPLANATION_SENTINEL.len())
            .any(|window| window == EXPLANATION_SENTINEL.as_bytes()),
        "the embedded executable must contain the explanation it prints"
    );

    let normal = run(&embedded_binary, &["normal-output"]);
    assert_success(&normal, "normal report output");
    let normal_text = format!(
        "{}{}",
        String::from_utf8_lossy(&normal.stdout),
        String::from_utf8_lossy(&normal.stderr)
    );
    assert!(normal_text.contains("normal output sentinel"));
    assert!(normal_text.contains("registry::alpha/E1000"));
    assert!(!normal_text.contains(EXPLANATION_SENTINEL));

    let governance = run(&embedded_binary, &["governance"]);
    assert_success(&governance, "catalog governance");
    assert_eq!(
        String::from_utf8_lossy(&governance.stdout).trim(),
        "governance=ok;catalog-coverage=5;markdown-examples=0"
    );

    let panic = run(&embedded_binary, &["panic-output"]);
    assert!(!panic.status.success(), "panic_report must terminate the child");
    let panic_stderr = String::from_utf8_lossy(&panic.stderr);
    assert!(panic_stderr.contains("panic output sentinel"));
    assert!(panic_stderr.contains("registry::alpha/E1000"));
    assert!(!panic_stderr.contains(EXPLANATION_SENTINEL));
}

#[test]
fn linked_registry_wasm_constructor_and_private_anchor_controls_run_in_wasmi() {
    for (variant, features, expected) in
        [("wasm-unanchored", &[][..], 0_i32), ("wasm-anchored", &["anchors"][..], 6_i32)]
    {
        let target_dir =
            cargo_build("registry-wasm-probe", variant, Some("wasm32-unknown-unknown"), features);
        let wasm_path = target_dir
            .join("wasm32-unknown-unknown")
            .join("release")
            .join("registry_wasm_probe.wasm");
        let wasm = fs::read(&wasm_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", wasm_path.display()));
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm[..]).expect("wasmi must validate the fixture");
        assert_eq!(module.imports().count(), 0, "{variant} must have zero imports");
        let export_names =
            module.exports().map(|export| export.name().to_owned()).collect::<Vec<_>>();
        assert!(
            export_names
                .iter()
                .all(|name| !name.contains("anchor") && name != "__wasm_call_ctors"),
            "{variant} leaked a contributor or constructor anchor: {export_names:?}"
        );

        let mut store = Store::new(&engine, ());
        let linker = Linker::new(&engine);
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .expect("zero-import fixture must instantiate");
        let initialize = instance
            .get_typed_func::<(), i32>(&store, "registry_initialize")
            .expect("initializer export");
        let raw_len = instance
            .get_typed_func::<(), i32>(&store, "registry_raw_inventory_len")
            .expect("raw inventory export");
        let registry_len = instance
            .get_typed_func::<(), i32>(&store, "registry_registry_len")
            .expect("registry length export");
        let registry_code = instance
            .get_typed_func::<i32, i32>(&store, "registry_registry_code")
            .expect("registry code export");

        assert_eq!(raw_len.call(&mut store, ()).unwrap(), 0);
        assert_eq!(registry_len.call(&mut store, ()).unwrap(), -1);
        assert_eq!(initialize.call(&mut store, ()).unwrap(), 1);
        assert_eq!(initialize.call(&mut store, ()).unwrap(), 1);
        assert_eq!(raw_len.call(&mut store, ()).unwrap(), expected);
        assert_eq!(registry_len.call(&mut store, ()).unwrap(), expected);

        if expected == 6 {
            let tags = (0..expected)
                .map(|index| registry_code.call(&mut store, index).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(tags, [0xa1000, 0xa2000, 0xa5000, 0xa4000, 0xb1000, 0xb3000]);
        } else {
            assert_eq!(registry_code.call(&mut store, 0).unwrap(), 0);
        }
    }
}
