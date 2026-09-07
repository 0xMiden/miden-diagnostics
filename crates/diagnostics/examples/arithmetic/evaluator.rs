use miden_diagnostics::{Report, Result, SourceId, SourceSpan};

use super::{
    diagnostics::{ArithmeticOverflow, DivisionByZero},
    syntax::{Expr, Operator},
};

/// Evaluate one valid syntax tree, failing fast on semantic errors.
pub fn evaluate(source: SourceId, expression: &Expr) -> Result<i64> {
    match expression {
        Expr::Number { value, .. } => Ok(*value),
        Expr::Binary {
            left,
            operator,
            operator_range,
            right,
            ..
        } => {
            let left = evaluate(source, left)?;
            let right_value = evaluate(source, right)?;
            if *operator == Operator::Divide && right_value == 0 {
                return Err(Report::new(DivisionByZero {
                    span: SourceSpan::session(source, right.range()),
                })
                .context("while evaluating the arithmetic expression"));
            }

            let value = match operator {
                Operator::Add => left.checked_add(right_value),
                Operator::Subtract => left.checked_sub(right_value),
                Operator::Multiply => left.checked_mul(right_value),
                Operator::Divide => left.checked_div(right_value),
            };
            value.ok_or_else(|| {
                Report::new(ArithmeticOverflow {
                    operation: operator.name(),
                    span: SourceSpan::session(source, *operator_range),
                })
                .context("while evaluating the arithmetic expression")
            })
        }
    }
}
