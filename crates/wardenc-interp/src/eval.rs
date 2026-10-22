use crate::value::{RuntimeError, Value};
use wardenc_syntax::ast::*;

pub type EvalResult = Result<Value, RuntimeError>;

fn err<T>(message: impl Into<String>, span: &Span) -> Result<T, RuntimeError> {
    Err(RuntimeError {
        message: message.into(),
        span: span.clone(),
    })
}

fn overflow(span: &Span) -> RuntimeError {
    RuntimeError {
        message: "integer overflow".to_string(),
        span: span.clone(),
    }
}

pub fn eval_expr(expr: &Node<Expr>) -> EvalResult {
    match &expr.kind {
        Expr::Int(n) => Ok(Value::Int(*n)),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Str(s) => Ok(Value::Str(s.clone())),
        Expr::Unary { op, operand } => {
            let v = eval_expr(operand)?;
            eval_unary(*op, v, &expr.span)
        }
        Expr::Binary { op, lhs, rhs } => eval_binary(*op, lhs, rhs, &expr.span),
        _ => err("this kind of expression isn't supported yet", &expr.span),
    }
}

fn eval_unary(op: UnaryOp, v: Value, span: &Span) -> EvalResult {
    match (op, v) {
        (UnaryOp::Neg, Value::Int(n)) => n
            .checked_neg()
            .map(Value::Int)
            .ok_or_else(|| overflow(span)),
        (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (UnaryOp::Neg, v) => err(format!("can't negate a {}", v.type_name()), span),
        (UnaryOp::Not, v) => err(format!("can't apply `!` to a {}", v.type_name()), span),
    }
}

fn eval_binary(op: BinaryOp, lhs: &Node<Expr>, rhs: &Node<Expr>, span: &Span) -> EvalResult {
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        return eval_logic(op, lhs, rhs);
    }

    let l = eval_expr(lhs)?;
    let r = eval_expr(rhs)?;
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => int_op(op, a, b, span),
        (Value::Bool(a), Value::Bool(b)) => eq_only(op, a == b, "bool", span),
        (Value::Str(a), Value::Str(b)) => eq_only(op, a == b, "str", span),
        (l, r) => err(
            format!(
                "can't use this operator on {} and {}",
                l.type_name(),
                r.type_name()
            ),
            span,
        ),
    }
}

fn eq_only(op: BinaryOp, equal: bool, ty: &str, span: &Span) -> EvalResult {
    match op {
        BinaryOp::Eq => Ok(Value::Bool(equal)),
        BinaryOp::NotEq => Ok(Value::Bool(!equal)),
        _ => err(format!("this operator doesn't work on {ty}s"), span),
    }
}

fn int_op(op: BinaryOp, a: i64, b: i64, span: &Span) -> EvalResult {
    let arith = |r: Option<i64>| r.map(Value::Int).ok_or_else(|| overflow(span));
    match op {
        BinaryOp::Add => arith(a.checked_add(b)),
        BinaryOp::Sub => arith(a.checked_sub(b)),
        BinaryOp::Mul => arith(a.checked_mul(b)),
        BinaryOp::Div | BinaryOp::Rem if b == 0 => err("division by zero", span),
        BinaryOp::Div => arith(a.checked_div(b)),
        BinaryOp::Rem => arith(a.checked_rem(b)),
        BinaryOp::Eq => Ok(Value::Bool(a == b)),
        BinaryOp::NotEq => Ok(Value::Bool(a != b)),
        BinaryOp::Lt => Ok(Value::Bool(a < b)),
        BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
        BinaryOp::Gt => Ok(Value::Bool(a > b)),
        BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
        BinaryOp::And | BinaryOp::Or => unreachable!("handled in eval_binary"),
    }
}

fn eval_logic(op: BinaryOp, lhs: &Node<Expr>, rhs: &Node<Expr>) -> EvalResult {
    let l = expect_bool(eval_expr(lhs)?, &lhs.span)?;
    match (op, l) {
        (BinaryOp::And, false) => return Ok(Value::Bool(false)),
        (BinaryOp::Or, true) => return Ok(Value::Bool(true)),
        _ => {}
    }
    let r = expect_bool(eval_expr(rhs)?, &rhs.span)?;
    Ok(Value::Bool(r))
}

fn expect_bool(v: Value, span: &Span) -> Result<bool, RuntimeError> {
    match v {
        Value::Bool(b) => Ok(b),
        other => err(
            format!("expected a bool, got a {}", other.type_name()),
            span,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wardenc_syntax::parser::parse;

    fn run(src: &str) -> EvalResult {
        let program = parse(&format!("fn main() {{ {src} }}")).expect("should parse");
        let Item::Fn(f) = &program.items[0].kind;
        let tail = f
            .body
            .kind
            .tail
            .as_ref()
            .expect("expected a tail expression");
        eval_expr(tail)
    }

    fn msg(src: &str) -> String {
        run(src).expect_err("should fail").message
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(run("1 + 2 * 3"), Ok(Value::Int(7)));
        assert_eq!(run("(1 + 2) * 3"), Ok(Value::Int(9)));
        assert_eq!(run("-2 + 5"), Ok(Value::Int(3)));
        assert_eq!(run("10 - 3 - 2"), Ok(Value::Int(5)));
        assert_eq!(run("17 % 5"), Ok(Value::Int(2)));
    }

    #[test]
    fn comparisons_and_logic() {
        assert_eq!(run("1 < 2 && 2 < 3"), Ok(Value::Bool(true)));
        assert_eq!(run("1 == 2 || 3 >= 3"), Ok(Value::Bool(true)));
        assert_eq!(run("!true"), Ok(Value::Bool(false)));
    }

    #[test]
    fn strings_and_bools_compare() {
        assert_eq!(run("\"a\" == \"a\""), Ok(Value::Bool(true)));
        assert_eq!(run("\"a\" != \"b\""), Ok(Value::Bool(true)));
        assert_eq!(run("true == false"), Ok(Value::Bool(false)));
    }

    #[test]
    fn logic_short_circuits() {
        assert_eq!(run("false && (1 / 0 == 0)"), Ok(Value::Bool(false)));
        assert_eq!(run("true || (1 / 0 == 0)"), Ok(Value::Bool(true)));
    }

    #[test]
    fn runtime_errors() {
        assert_eq!(msg("1 / 0"), "division by zero");
        assert_eq!(msg("1 % 0"), "division by zero");
        assert_eq!(msg("9223372036854775807 + 1"), "integer overflow");
        assert_eq!(msg("1 + true"), "can't use this operator on int and bool");
        assert_eq!(msg("-true"), "can't negate a bool");
        assert_eq!(msg("1 && true"), "expected a bool, got a int");
    }
}
