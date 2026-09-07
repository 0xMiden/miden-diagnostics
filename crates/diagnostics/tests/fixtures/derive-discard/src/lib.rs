#![no_std]
#![forbid(unsafe_code)]

#[derive(Debug, diag_runtime::Diagnostic)]
#[diagnostic(
    code = "derive-fixture/discarded",
    summary = "discarded nonexistent explanation",
    explanation = include_str!("this-file-intentionally-does-not-exist.md"),
    message = "discarded"
)]
pub struct Discarded;

#[cfg(test)]
mod tests {
    use diag_runtime::Diagnostic as _;
    use diag_runtime::Explanation;

    use super::*;

    #[test]
    fn disabled_facade_discards_nonexistent_include_tokens() {
        assert_eq!(
            Discarded.descriptor().unwrap().explanation,
            Explanation::NotEmbedded
        );
    }
}
