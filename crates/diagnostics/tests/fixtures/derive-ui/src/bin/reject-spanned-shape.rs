use renamed_diagnostics::{SourceSpan, Spanned};

#[derive(Spanned)]
#[spanned(transparent, forward(span))]
struct ConflictingForwarding {
    span: SourceSpan,
}

#[derive(Spanned)]
struct OptionalSpan {
    #[span]
    span: Option<SourceSpan>,
}

#[derive(Spanned)]
#[spanned(forward(0))]
enum TypeLevelForwarding {
    Value(SourceSpan),
}

fn main() {}
