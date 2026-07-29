use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use wasmi::{Engine, Linker, Module, Store};

#[test]
fn linked_no_std_wasm_executes_with_wasmi() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture_manifest = manifest_dir.join("tests/fixtures/linked-no-std/Cargo.toml");
    let target_dir = manifest_dir
        .join("../../target/wasmi-linked-no-std")
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join("../../target/wasmi-linked-no-std"));

    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--manifest-path",
            fixture_manifest.to_str().expect("the fixture path must be UTF-8"),
            "--target",
            "wasm32-unknown-unknown",
            "--release",
        ])
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("cargo must launch for the linked no-std fixture");
    assert!(
        output.status.success(),
        "linked no-std Wasm build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let wasm_path: PathBuf = target_dir
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("linked-no-std.wasm");
    let wasm = fs::read(&wasm_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", wasm_path.display()));

    let engine = Engine::default();
    let module = Module::new(&engine, &wasm[..]).expect("wasmi must validate the fixture");
    assert_eq!(module.imports().count(), 0, "the portable fixture must remain self-contained");
    let mut store = Store::new(&engine, ());
    let linker = Linker::new(&engine);
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .expect("the no-import fixture must instantiate");
    let start = instance
        .get_typed_func::<(), i32>(&store, "_start")
        .expect("the fixture must export _start with the expected signature");
    assert_eq!(start.call(&mut store, ()).expect("_start must execute"), 8);
    let registry_probe = instance
        .get_typed_func::<(), i32>(&store, "registry_probe")
        .expect("the fixture must export the registry probe");
    assert_eq!(
        registry_probe
            .call(&mut store, ())
            .expect("the portable registry probe must execute"),
        0xff,
        "every portable explicit-registry invariant must pass",
    );
}
