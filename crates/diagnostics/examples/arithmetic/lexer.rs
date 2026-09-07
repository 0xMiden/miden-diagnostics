use miden_diagnostics::{DiagnosticSink, OwnedDiagnostic, SourceId, SourceSpan, TextRange};

use super::{
    diagnostics::{IntegerOutOfRange, LeadingZeros, UnexpectedCharacter},
    syntax::{Token, TokenKind},
};

pub struct Lexed {
    pub tokens: Vec<Token>,
    pub had_errors: bool,
}

pub fn lex(source: SourceId, input: &str, diagnostics: &mut dyn DiagnosticSink) -> Lexed {
    let mut tokens = Vec::new();
    let mut characters = input.char_indices().peekable();
    let mut had_errors = false;

    while let Some((start, character)) = characters.next() {
        if character.is_whitespace() {
            continue;
        }

        if character.is_ascii_digit() {
            let mut end = start + character.len_utf8();
            while let Some((offset, next)) = characters.peek().copied() {
                if !next.is_ascii_digit() {
                    break;
                }
                let _ = characters.next();
                end = offset + next.len_utf8();
            }
            let literal = &input[start..end];
            let range = checked_range(start, end);
            if literal.len() > 1 && literal.starts_with('0') {
                let replacement = literal.trim_start_matches('0');
                let replacement = if replacement.is_empty() {
                    "0"
                } else {
                    replacement
                };
                let span = SourceSpan::session(source, range);
                let _ = diagnostics.push(OwnedDiagnostic::new(LeadingZeros {
                    literal: literal.into(),
                    replacement: replacement.into(),
                    label: span,
                    suggestion: span,
                }));
            }
            match literal.parse::<i64>() {
                Ok(value) => tokens.push(Token {
                    kind: TokenKind::Number(value),
                    range,
                }),
                Err(_) => {
                    had_errors = true;
                    let _ = diagnostics.push(OwnedDiagnostic::new(IntegerOutOfRange {
                        literal: literal.into(),
                        span: SourceSpan::session(source, range),
                    }));
                }
            }
            continue;
        }

        let end = start + character.len_utf8();
        let range = checked_range(start, end);
        let kind = match character {
            '+' => Some(TokenKind::Plus),
            '-' => Some(TokenKind::Minus),
            '*' => Some(TokenKind::Star),
            '/' => Some(TokenKind::Slash),
            '(' => Some(TokenKind::LeftParen),
            ')' => Some(TokenKind::RightParen),
            _ => None,
        };
        if let Some(kind) = kind {
            tokens.push(Token { kind, range });
        } else {
            had_errors = true;
            let _ = diagnostics.push(OwnedDiagnostic::new(UnexpectedCharacter {
                character,
                span: SourceSpan::session(source, range),
            }));
        }
    }

    let end = checked_range(input.len(), input.len());
    tokens.push(Token {
        kind: TokenKind::End,
        range: end,
    });
    Lexed { tokens, had_errors }
}

fn checked_range(start: usize, end: usize) -> TextRange {
    TextRange::try_from_usize(start, end)
        .expect("the application validates the source length before lexing")
}
