use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

const REGISTRY_LISTING: &str = "\
arithmetic/E0001: unexpected input character
arithmetic/E0002: integer literal is outside the i64 range
arithmetic/W0001: integer literal has redundant leading zeros
arithmetic/E1001: expected an arithmetic expression
arithmetic/E1002: unclosed parenthesized expression
arithmetic/E1003: unexpected token after the expression
arithmetic/E2001: division by zero
arithmetic/E2002: arithmetic overflow
";

#[test]
fn arithmetic_example_is_a_cohesive_executable_walkthrough() {
    let target = example_target();
    build_example(&target);
    let binary = example_binary(&target);

    let success = run(&binary, "1 + 2 * 3");
    assert!(success.status.success());
    assert_eq!(text(&success.stdout), "result: 7\n");
    assert_eq!(success.stderr, b"");

    let warning = run(&binary, "007 + 1");
    assert!(warning.status.success());
    assert_eq!(text(&warning.stdout), "result: 8\n");
    let warning_stderr = text(&warning.stderr);
    assert_eq!(warning_stderr.matches("warning[arithmetic/W0001]").count(), 1);
    assert!(warning_stderr.contains("leading zeros are unnecessary"));
    assert!(warning_stderr.contains("applicability: machine-applicable"));
    assert!(!warning_stderr.contains("error["));

    let lexical = run(&binary, "@ + #");
    assert_failure(&lexical);
    assert_eq!(lexical.stdout, b"");
    let lexical_stderr = text(&lexical.stderr);
    assert_eq!(lexical_stderr.matches("error[arithmetic/E0001]").count(), 2);
    let first = lexical_stderr.find("unexpected character `@`").expect("first lexical error");
    let second = lexical_stderr.find("unexpected character `#`").expect("second lexical error");
    assert!(first < second, "lexical diagnostics changed insertion order");
    assert!(!lexical_stderr.contains("arithmetic/E100"));
    assert_eq!(lexical.stderr, run(&binary, "@ + #").stderr);

    let out_of_range = run(&binary, "999999999999999999999999");
    assert_failure(&out_of_range);
    assert_eq!(text(&out_of_range.stderr).matches("error[arithmetic/E0002]").count(), 1);
    assert!(!text(&out_of_range.stderr).contains("arithmetic/E100"));

    let unclosed = run(&binary, "(1 + 2");
    assert_failure(&unclosed);
    assert_eq!(unclosed.stdout, b"");
    let unclosed_stderr = text(&unclosed.stderr);
    assert_eq!(unclosed_stderr.matches("error[arithmetic/E1002]").count(), 1);
    assert!(unclosed_stderr.contains("insert the missing closing parenthesis"));
    assert!(!unclosed_stderr.contains("result:"));

    let expected = run(&binary, "1 +");
    assert_failure(&expected);
    assert_eq!(text(&expected.stderr).matches("error[arithmetic/E1001]").count(), 1);

    let trailing = run(&binary, "1 2");
    assert_failure(&trailing);
    assert_eq!(text(&trailing.stderr).matches("error[arithmetic/E1003]").count(), 1);

    let division = run(&binary, "10 / (3 - 3)");
    assert_failure(&division);
    assert_eq!(division.stdout, b"");
    let division_stderr = text(&division.stderr);
    assert_eq!(division_stderr.matches("error[arithmetic/E2001]").count(), 1);
    assert!(division_stderr.contains("while evaluating the arithmetic expression"));
    assert!(!division_stderr.contains("arithmetic/E100"));

    let overflow = run(&binary, "9223372036854775807 + 1");
    assert_failure(&overflow);
    assert_eq!(text(&overflow.stderr).matches("error[arithmetic/E2002]").count(), 1);

    let registry = run(&binary, "--list-diagnostics");
    assert!(registry.status.success());
    assert_eq!(text(&registry.stdout), REGISTRY_LISTING);
    assert_eq!(registry.stderr, b"");
}

fn example_target() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/arithmetic-example")
}

fn build_example(target: &Path) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["build", "--color=never", "--manifest-path"])
        .arg(manifest)
        .args(["--no-default-features", "--features", "std,derive", "--example", "arithmetic"])
        .env("CARGO_TARGET_DIR", target)
        .output()
        .expect("arithmetic example build must launch");
    assert!(
        output.status.success(),
        "arithmetic example build failed\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

fn example_binary(target: &Path) -> PathBuf {
    target
        .join("debug")
        .join("examples")
        .join(format!("arithmetic{}", std::env::consts::EXE_SUFFIX))
}

fn run(binary: &Path, argument: &str) -> Output {
    Command::new(binary)
        .arg(argument)
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("COLUMNS", "120")
        .output()
        .unwrap_or_else(|error| panic!("failed to run {}: {error}", binary.display()))
}

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "process unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("arithmetic example output must be UTF-8")
}
