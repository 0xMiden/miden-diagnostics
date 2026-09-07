use miden_diagnostics::TextRange;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenKind {
    Number(i64),
    Plus,
    Minus,
    Star,
    Slash,
    LeftParen,
    RightParen,
    End,
}

impl TokenKind {
    pub fn description(self) -> String {
        match self {
            Self::Number(value) => format!("integer `{value}`"),
            Self::Plus => "`+`".into(),
            Self::Minus => "`-`".into(),
            Self::Star => "`*`".into(),
            Self::Slash => "`/`".into(),
            Self::LeftParen => "`(`".into(),
            Self::RightParen => "`)`".into(),
            Self::End => "the end of the input".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub range: TextRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl Operator {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Add => "addition",
            Self::Subtract => "subtraction",
            Self::Multiply => "multiplication",
            Self::Divide => "division",
        }
    }
}

#[derive(Debug)]
pub enum Expr {
    Number {
        value: i64,
        range: TextRange,
    },
    Binary {
        left: Box<Expr>,
        operator: Operator,
        operator_range: TextRange,
        right: Box<Expr>,
        range: TextRange,
    },
}

impl Expr {
    pub const fn range(&self) -> TextRange {
        match self {
            Self::Number { range, .. } | Self::Binary { range, .. } => *range,
        }
    }
}
