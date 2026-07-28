fn main() {
    let _ = renamed_diagnostics::diagnostic! {
        severity: Error,
        code: "missing-separator",
        message: "invalid",
    };
}
