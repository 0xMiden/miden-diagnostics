use miden_diagnostics::{
    DiagnosticCollector, DiagnosticSink, Outcome, OwnedDiagnostic, SourceId, SourceSpan, TextRange,
};

use super::{
    diagnostics::{ExpectedExpression, UnclosedParenthesis, UnexpectedTrailingToken},
    lexer,
    syntax::{Expr, Operator, Token, TokenKind},
};

/// Parse the complete input while retaining every warning or error produced
/// by the front end.
pub fn parse(source: SourceId, input: &str) -> Outcome<Option<Expr>> {
    let mut diagnostics = DiagnosticCollector::new();
    let lexed = lexer::lex(source, input, &mut diagnostics);
    let expression = if lexed.had_errors {
        None
    } else {
        Parser::new(source, &lexed.tokens, &mut diagnostics).parse()
    };
    Outcome {
        value: expression,
        diagnostics: diagnostics.finish(),
    }
}

struct Parser<'tokens, 'sink> {
    source: SourceId,
    tokens: &'tokens [Token],
    current: usize,
    diagnostics: &'sink mut dyn DiagnosticSink,
}

impl<'tokens, 'sink> Parser<'tokens, 'sink> {
    fn new(
        source: SourceId,
        tokens: &'tokens [Token],
        diagnostics: &'sink mut dyn DiagnosticSink,
    ) -> Self {
        Self {
            source,
            tokens,
            current: 0,
            diagnostics,
        }
    }

    fn parse(mut self) -> Option<Expr> {
        let expression = self.parse_sum();
        while self.current().kind != TokenKind::End {
            let token = self.advance();
            let _ = self.diagnostics.push(OwnedDiagnostic::new(UnexpectedTrailingToken {
                found: token.kind.description(),
                span: self.span(token.range),
            }));
        }
        expression
    }

    fn parse_sum(&mut self) -> Option<Expr> {
        let mut expression = self.parse_product()?;
        loop {
            let operator = match self.current().kind {
                TokenKind::Plus => Operator::Add,
                TokenKind::Minus => Operator::Subtract,
                _ => break,
            };
            let operator_range = self.advance().range;
            let right = self.parse_product()?;
            expression = binary(expression, operator, operator_range, right);
        }
        Some(expression)
    }

    fn parse_product(&mut self) -> Option<Expr> {
        let mut expression = self.parse_primary()?;
        loop {
            let operator = match self.current().kind {
                TokenKind::Star => Operator::Multiply,
                TokenKind::Slash => Operator::Divide,
                _ => break,
            };
            let operator_range = self.advance().range;
            let right = self.parse_primary()?;
            expression = binary(expression, operator, operator_range, right);
        }
        Some(expression)
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        let token = self.advance();
        match token.kind {
            TokenKind::Number(value) => Some(Expr::Number {
                value,
                range: token.range,
            }),
            TokenKind::LeftParen => {
                let expression = self.parse_sum()?;
                if self.current().kind == TokenKind::RightParen {
                    let closing = self.advance();
                    Some(with_range(
                        expression,
                        TextRange::new(token.range.start(), closing.range.end())
                            .expect("tokens remain in source order"),
                    ))
                } else {
                    let insertion = self.current().range;
                    let insertion_span = self.span(insertion);
                    let _ = self.diagnostics.push(OwnedDiagnostic::new(UnclosedParenthesis {
                        opened: self.span(token.range),
                        insertion_label: insertion_span,
                        insertion: insertion_span,
                    }));
                    Some(expression)
                }
            }
            other => {
                let _ = self.diagnostics.push(OwnedDiagnostic::new(ExpectedExpression {
                    found: other.description(),
                    span: self.span(token.range),
                }));
                None
            }
        }
    }

    fn current(&self) -> Token {
        self.tokens[self.current]
    }

    fn advance(&mut self) -> Token {
        let token = self.current();
        if token.kind != TokenKind::End {
            self.current += 1;
        }
        token
    }

    const fn span(&self, range: TextRange) -> SourceSpan {
        SourceSpan::session(self.source, range)
    }
}

fn binary(left: Expr, operator: Operator, operator_range: TextRange, right: Expr) -> Expr {
    let range = TextRange::new(left.range().start(), right.range().end())
        .expect("parsed expression ranges remain in source order");
    Expr::Binary {
        left: Box::new(left),
        operator,
        operator_range,
        right: Box::new(right),
        range,
    }
}

fn with_range(expression: Expr, range: TextRange) -> Expr {
    match expression {
        Expr::Number { value, .. } => Expr::Number { value, range },
        Expr::Binary {
            left,
            operator,
            operator_range,
            right,
            ..
        } => Expr::Binary {
            left,
            operator,
            operator_range,
            right,
            range,
        },
    }
}
