fn main() {
    let _ = renamed_diagnostics::report! {
        severity: Hint,
        message: "hint",
    };
}
