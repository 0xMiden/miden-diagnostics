#[cfg(unix)]
use std::{
    os::{fd::OwnedFd, unix::net::UnixDatagram},
    process::Stdio,
};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

const RICH_REPORT: &str = "\
error[std/E_REPORT]: could not compile module
 --> attached.masm:2:5
  |
1 | begin
2 |     broken
  |     ^^^^^^ unexpected token
3 | end
  |
  = note: context: while lowering component
  = help: replace `broken`
";

#[test]
fn std_runtime_contracts() {
    let fixture = fixture_manifest();
    let target = fixture_target();
    build_fixture(&fixture, &target, false);
    build_fixture(&fixture, &target, true);

    let result = binary(&target, false, "result-report");
    let exit = binary(&target, false, "exit-outcome");
    let panic = binary(&target, false, "panic-hook");
    let abort_panic = binary(&target, true, "panic-hook");

    let rich = run(&result, ["rich"], []);
    assert_failure(&rich);
    assert_eq!(rich.stdout, b"");
    assert_eq!(text(&rich.stderr), format!("Error: {RICH_REPORT}"));
    assert!(!text(&rich.stderr).contains("PHASE4_LONG_EXPLANATION_MUST_NOT_RENDER"));
    assert!(!rich.stderr.contains(&0x1b));
    let rich_repeat = run(&result, ["rich"], []);
    assert_eq!(rich.stderr, rich_repeat.stderr);

    let stdout_emitter = run(&result, ["stdout-emitter"], []);
    assert!(stdout_emitter.status.success());
    assert_eq!(text(&stdout_emitter.stdout), RICH_REPORT);
    assert_eq!(stdout_emitter.stderr, b"");

    let missing = run(&result, ["missing-source"], []);
    assert_failure(&missing);
    assert_eq!(
        text(&missing.stderr),
        "Error: error[std/E_MISSING]: source is unavailable\nnote: diagnostic rendering degraded: \
         missing source\n"
    );

    let prepare = run(&result, ["prepare-failure"], []);
    assert_failure(&prepare);
    assert_eq!(
        text(&prepare.stderr),
        "Error: error: diagnostic preparation failed; rich output unavailable\n"
    );

    let empty = run(&exit, ["empty"], []);
    assert!(empty.status.success());
    assert_eq!(empty.stderr, b"");
    assert_eq!(empty.stdout, b"");

    let warning = run(&exit, ["warning"], []);
    assert!(warning.status.success());
    assert_eq!(text(&warning.stderr), "warning: warning-only\n");

    #[cfg(unix)]
    {
        let (broken, peer) =
            UnixDatagram::pair().expect("Unix datagram pair must be available for I/O failure");
        drop(peer);
        let broken: OwnedFd = broken.into();
        let mut child = Command::new(&exit)
            .arg("warning")
            .stdout(Stdio::piped())
            .stderr(Stdio::from(broken))
            .spawn()
            .expect("warning process with disconnected stderr must launch");
        let broken_stderr = child
            .wait()
            .map(|status| Output {
                status,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
            .expect("warning process with disconnected stderr must finish");
        assert_failure(&broken_stderr);
    }

    let error = run(&exit, ["error"], []);
    assert_failure(&error);
    assert_eq!(text(&error.stderr), "error: error-only\n");

    let warning_failure = run(&exit, ["warnings-as-errors"], []);
    assert_failure(&warning_failure);
    assert_eq!(text(&warning_failure.stderr), "warning: warning-is-failure\n");

    let all = run(&exit, ["all"], []);
    assert_failure(&all);
    assert_eq!(
        text(&all.stderr),
        "hint: first-hint\nerror: second-error\nwarning: third-warning\ninfo: fourth-info\n"
    );

    let attached = run(&exit, ["attached-source"], []);
    assert_failure(&attached);
    assert_eq!(text(&attached.stderr), RICH_REPORT);

    let session = run(&exit, ["session-source"], []);
    assert_failure(&session);
    assert!(text(&session.stderr).contains(" --> session.masm:2:5\n"));

    let degraded_warning = run(&exit, ["missing-source-warning"], []);
    assert!(degraded_warning.status.success());
    assert_eq!(
        text(&degraded_warning.stderr),
        "warning[std/E_MISSING]: source is unavailable\nnote: diagnostic rendering degraded: \
         missing source\n"
    );

    let exit_prepare = run(&exit, ["prepare-failure"], []);
    assert_failure(&exit_prepare);
    assert_eq!(
        text(&exit_prepare.stderr),
        "error: diagnostic preparation failed; rich output unavailable\n"
    );

    for columns in ["0", "not-a-number", "999999999999999999999999"] {
        let automatic = run(
            &exit,
            ["attached-source"],
            [("TERM", "dumb"), ("NO_COLOR", "1"), ("COLUMNS", columns)],
        );
        assert_eq!(text(&automatic.stderr), RICH_REPORT);
        assert!(!automatic.stderr.contains(&0x1b));
    }

    let explicit = run(
        &exit,
        ["explicit-terminal"],
        [("TERM", "dumb"), ("NO_COLOR", "1"), ("COLUMNS", "invalid")],
    );
    assert_failure(&explicit);
    let explicit_text = text(&explicit.stderr);
    assert!(explicit_text.contains("\u{1b}["));
    assert!(
        explicit_text.contains("\u{1b}]8;;https://example.com/std/E_REPORT"),
        "explicit hyperlink override was not honored:\n{explicit_text}"
    );
    assert!(!explicit.stderr.is_ascii(), "explicit Unicode override was not honored");

    let typed = run(&panic, ["typed"], []);
    assert_eq!(typed.status.code(), Some(101));
    assert_eq!(text(&typed.stdout), "profile=unwind\n");
    assert_eq!(text(&typed.stderr), RICH_REPORT);
    assert_typed_panic_is_clean(&typed);

    let caught = run(&panic, ["typed-caught"], []);
    assert!(caught.status.success());
    assert_eq!(text(&caught.stdout), "caught=true calls=1\n");
    assert_eq!(text(&caught.stderr), "error: counted diagnostic\n");

    let unrelated = run(&panic, ["unrelated"], []);
    assert_eq!(unrelated.status.code(), Some(101));
    assert_eq!(text(&unrelated.stdout), "profile=unwind\n");
    assert_eq!(text(&unrelated.stderr), "previous:unrelated\n");

    let formatted = run(&panic, ["formatted-report"], []);
    assert_failure(&formatted);
    assert_eq!(text(&formatted.stderr), "previous:could not compile module\n");

    let installation = run(&panic, ["install-state"], []);
    assert!(installation.status.success());
    let installation_stdout = text(&installation.stdout);
    assert!(installation_stdout.contains("first=Ok(())\nsame=Ok(())\n"));
    assert!(installation_stdout.contains(
        "different=Err(IncompatibleOptions { installed: PanicHookOptions { terminal_policy: \
         TerminalPolicy { styled: Auto, unicode: Auto, hyperlinks: Auto, width: Auto } }, \
         requested: PanicHookOptions { terminal_policy: TerminalPolicy { styled: Always, unicode: \
         Auto, hyperlinks: Auto, width: Auto } } })\n"
    ));
    assert_eq!(text(&installation.stderr), "previous:ordinary\n");

    let concurrent_same = run(&panic, ["install-concurrent-same"], []);
    assert!(concurrent_same.status.success());
    assert_eq!(text(&concurrent_same.stdout), "same-ok=8\n");
    assert_eq!(concurrent_same.stderr, b"");

    let concurrent_mixed = run(&panic, ["install-concurrent-mixed"], []);
    assert!(concurrent_mixed.status.success());
    let mixed = text(&concurrent_mixed.stdout);
    assert!(
        mixed == "mixed-winner=auto winner-ok=4 incompatible=4\n"
            || mixed == "mixed-winner=styled winner-ok=4 incompatible=4\n",
        "unexpected mixed installation result: {mixed:?}"
    );
    assert_eq!(concurrent_mixed.stderr, b"");

    let delegated_location = run(&panic, ["delegation-location"], []);
    assert_failure(&delegated_location);
    let delegated_location = text(&delegated_location.stderr);
    assert!(
        delegated_location.starts_with("delegated-location=src/bin/panic-hook.rs:"),
        "previous hook did not receive the original panic location: {delegated_location:?}"
    );
    assert_eq!(delegated_location.lines().count(), 1);

    let installing_during_panic = run(&panic, ["install-while-panicking"], []);
    assert_failure(&installing_during_panic);
    assert_eq!(text(&installing_during_panic.stderr), "install-during=Err(PanickingThread)\n");

    let pre_render_panic = run(&panic, ["pre-render-panics"], []);
    assert_failure(&pre_render_panic);
    assert_eq!(text(&pre_render_panic.stderr), "previous:message formatter panicked\n");

    let panic_prepare = run(&panic, ["prepare-failure"], []);
    assert_failure(&panic_prepare);
    assert_eq!(
        text(&panic_prepare.stderr),
        "error: diagnostic preparation failed; rich output unavailable\n"
    );

    let uninstalled = run(&panic, ["uninstalled"], []);
    assert_failure(&uninstalled);
    assert_eq!(text(&uninstalled.stderr), format!("previous:{RICH_REPORT}\n"));

    let during_unwind = run(&panic, ["panic-during-unwind"], []);
    assert_failure(&during_unwind);
    assert_eq!(
        text(&during_unwind.stderr),
        "previous:begin unwind\nfatal: diagnostic panic report unavailable\n"
    );

    let typed_abort = run(&abort_panic, ["typed"], []);
    assert_failure(&typed_abort);
    assert_eq!(text(&typed_abort.stdout), "profile=abort\n");
    assert_eq!(text(&typed_abort.stderr), RICH_REPORT);
    assert_typed_panic_is_clean(&typed_abort);

    let unrelated_abort = run(&abort_panic, ["unrelated"], []);
    assert_failure(&unrelated_abort);
    assert_eq!(text(&unrelated_abort.stdout), "profile=abort\n");
    assert_eq!(text(&unrelated_abort.stderr), "previous:unrelated\n");
}

fn fixture_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/std-runtime/Cargo.toml")
}

fn fixture_target() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/std-runtime")
}

fn build_fixture(manifest: &Path, target: &Path, release: bool) {
    let mut command = Command::new(env!("CARGO"));
    command
        .args(["build", "--color=never", "--manifest-path"])
        .arg(manifest)
        .args(["--locked"])
        .env("CARGO_TARGET_DIR", target);
    if release {
        command.args(["--bin", "panic-hook", "--release"]);
    } else {
        command.arg("--bins");
    }
    let output = command.output().expect("fixture build must launch");
    assert!(
        output.status.success(),
        "fixture build failed\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

fn binary(target: &Path, release: bool, name: &str) -> PathBuf {
    target
        .join(if release { "release" } else { "debug" })
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn run<const A: usize, const E: usize>(
    binary: &Path,
    args: [&str; A],
    environment: [(&str, &str); E],
) -> Output {
    let mut command = Command::new(binary);
    command.args(args);
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().unwrap_or_else(|error| {
        panic!("failed to run {}: {error}", binary.display());
    })
}

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "process unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

fn assert_typed_panic_is_clean(output: &Output) {
    let stderr = text(&output.stderr);
    for forbidden in [
        "previous:",
        "panicked at",
        "Box<dyn Any>",
        "PHASE4_LONG_EXPLANATION_MUST_NOT_RENDER",
    ] {
        assert!(!stderr.contains(forbidden), "typed panic contained {forbidden:?}:\n{stderr}");
    }
    assert!(!output.stderr.contains(&0x1b));
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("fixture output must be UTF-8")
}
