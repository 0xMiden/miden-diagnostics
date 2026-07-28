#[derive(Debug, renamed_diagnostics::Diagnostic)]
#[diagnostic(message = "one", message = "two")]
struct Duplicate;

#[derive(Debug, renamed_diagnostics::Diagnostic)]
#[diagnostic(unknown = "value")]
struct Unknown;

fn main() {}
