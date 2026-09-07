use core::fmt;

/// The semantic severity of a diagnostic occurrence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// Standardized semantic tags
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum DiagnosticTag {
    Unnecessary,
    Deprecated,
}

/// A static, namespaced diagnostic code.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DiagnosticCode {
    pub namespace: &'static str,
    pub code: &'static str,
}

impl DiagnosticCode {
    pub const fn as_ref(&self) -> DiagnosticCodeRef<'_> {
        DiagnosticCodeRef {
            namespace: self.namespace,
            code: self.code,
        }
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(formatter)
    }
}

/// A borrowed diagnostic code used by runtime-only diagnostics.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DiagnosticCodeRef<'a> {
    pub namespace: &'a str,
    pub code: &'a str,
}

impl fmt::Display for DiagnosticCodeRef<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.namespace, self.code)
    }
}

/// Availability of a descriptor's long-form explanation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Explanation {
    NotProvided,
    NotEmbedded,
    Embedded(&'static str),
}

/// The definition site of a static diagnostic descriptor.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DescriptorOrigin {
    pub module_path: &'static str,
    pub file: &'static str,
    pub line: u32,
}

/// Static identity and documentation for a diagnostic kind.
#[derive(Debug, Eq, PartialEq)]
pub struct DiagnosticDescriptor {
    pub code: DiagnosticCode,
    pub summary: &'static str,
    pub default_severity: Severity,
    pub explanation: Explanation,
    pub documentation_url: Option<&'static str>,
    pub tags: &'static [DiagnosticTag],
    pub origin: DescriptorOrigin,
}

/// Lightweight semantic metadata passed to a failure policy.
#[derive(Clone, Copy, Debug)]
pub struct DiagnosticMetadata<'a> {
    pub descriptor: Option<&'static DiagnosticDescriptor>,
    pub code: Option<DiagnosticCodeRef<'a>>,
    pub severity: Severity,
    pub tags: &'a [DiagnosticTag],
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn code_display_and_explanation_states_are_explicit() {
        let code = DiagnosticCode {
            namespace: "miden::parser",
            code: "E0001",
        };
        assert_eq!(code.to_string(), "miden::parser/E0001");
        assert_eq!(code.as_ref().to_string(), "miden::parser/E0001");
        assert_ne!(Explanation::NotProvided, Explanation::NotEmbedded);
        assert_ne!(Explanation::NotEmbedded, Explanation::Embedded("sentinel"));
    }

    #[test]
    fn all_four_severities_are_copyable_values() {
        let values = [Severity::Error, Severity::Warning, Severity::Info, Severity::Hint];
        assert_eq!(values, values);
    }
}
