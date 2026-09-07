use miden_diagnostics::{Diagnostic, SourceSpan, StaticRegistry};

miden_diagnostics::diagnostic_codes! {
    namespace = "arithmetic";
    catalog = DIAGNOSTICS;

    pub E0001 {
        summary: "unexpected input character",
        severity: Error,
        explanation: "The arithmetic grammar accepts decimal integers, `+`, `-`, `*`, `/`, and parentheses.",
    }

    pub E0002 {
        summary: "integer literal is outside the i64 range",
        severity: Error,
    }

    pub W0001 {
        summary: "integer literal has redundant leading zeros",
        severity: Warning,
    }

    pub E1001 {
        summary: "expected an arithmetic expression",
        severity: Error,
    }

    pub E1002 {
        summary: "unclosed parenthesized expression",
        severity: Error,
    }

    pub E1003 {
        summary: "unexpected token after the expression",
        severity: Error,
    }

    pub E2001 {
        summary: "division by zero",
        severity: Error,
    }

    pub E2002 {
        summary: "arithmetic overflow",
        severity: Error,
    }
}

static CATALOGS: &[&[&miden_diagnostics::DiagnosticDescriptor]] = &[DIAGNOSTICS];

pub static REGISTRY: StaticRegistry<'static> = StaticRegistry::from_slices(CATALOGS);

#[derive(Debug, Diagnostic)]
#[diagnostic(
    descriptor = E0001,
    message = "unexpected character `{character}`",
    help = "expected a decimal integer, an operator, or parentheses"
)]
pub struct UnexpectedCharacter {
    pub character: char,
    #[label(primary, "this character is not part of the arithmetic grammar")]
    pub span: SourceSpan,
}

#[derive(Debug, Diagnostic)]
#[diagnostic(
    descriptor = E0002,
    message = "integer literal `{literal}` is outside the supported range"
)]
pub struct IntegerOutOfRange {
    pub literal: String,
    #[label(primary, "this value does not fit in an i64")]
    pub span: SourceSpan,
}

#[derive(Debug, Diagnostic)]
#[diagnostic(
    descriptor = W0001,
    message = "integer literal `{literal}` has redundant leading zeros"
)]
pub struct LeadingZeros {
    pub literal: String,
    pub replacement: String,
    #[label(primary, "leading zeros are unnecessary")]
    pub label: SourceSpan,
    #[suggestion(
        "write the canonical decimal literal",
        replacement = "{replacement}",
        applicability = MachineApplicable
    )]
    pub suggestion: SourceSpan,
}

#[derive(Debug, Diagnostic)]
#[diagnostic(
    descriptor = E1001,
    message = "expected an expression, found {found}"
)]
pub struct ExpectedExpression {
    pub found: String,
    #[label(primary, "an integer or `(` was expected here")]
    pub span: SourceSpan,
}

#[derive(Debug, Diagnostic)]
#[diagnostic(
    descriptor = E1002,
    message = "parenthesized expression is missing its closing `)`"
)]
pub struct UnclosedParenthesis {
    #[label("this `(` opens the expression")]
    pub opened: SourceSpan,
    #[label(primary, "expected `)` here")]
    pub insertion_label: SourceSpan,
    #[suggestion(
        "insert the missing closing parenthesis",
        replacement = ")",
        applicability = MachineApplicable
    )]
    pub insertion: SourceSpan,
}

#[derive(Debug, Diagnostic)]
#[diagnostic(
    descriptor = E1003,
    message = "unexpected token {found} after the expression"
)]
pub struct UnexpectedTrailingToken {
    pub found: String,
    #[label(primary, "the complete expression ends before this token")]
    pub span: SourceSpan,
}

#[derive(Debug, Diagnostic)]
#[diagnostic(
    descriptor = E2001,
    message = "cannot divide by zero",
    help = "change the divisor so that it evaluates to a nonzero value"
)]
pub struct DivisionByZero {
    #[label(primary, "this divisor evaluates to zero")]
    pub span: SourceSpan,
}

#[derive(Debug, Diagnostic)]
#[diagnostic(
    descriptor = E2002,
    message = "{operation} overflows the i64 range"
)]
pub struct ArithmeticOverflow {
    pub operation: &'static str,
    #[label(primary, "overflow occurs at this operator")]
    pub span: SourceSpan,
}
