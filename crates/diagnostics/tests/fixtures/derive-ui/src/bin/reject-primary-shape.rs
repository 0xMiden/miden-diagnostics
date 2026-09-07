use renamed_diagnostics::SourceSpan;

#[derive(Debug, renamed_diagnostics::Diagnostic)]
#[diagnostic(message = "two primaries")]
struct TwoPrimaries {
    #[label(primary)]
    first: SourceSpan,
    #[label(primary)]
    second: SourceSpan,
}

#[derive(Debug, renamed_diagnostics::Diagnostic)]
#[diagnostic(message = "primary collection")]
struct PrimaryCollection {
    #[label(primary)]
    spans: alloc::vec::Vec<SourceSpan>,
}

#[derive(Debug, renamed_diagnostics::Diagnostic)]
#[diagnostic(message = "rich source collection")]
struct RichSourceCollection {
    #[diagnostic_source]
    sources: alloc::vec::Vec<PrimaryCollection>,
}

extern crate alloc;

fn main() {}
