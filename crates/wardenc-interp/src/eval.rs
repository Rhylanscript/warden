use crate::env::{AssignError, Env};
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

// --- blocks and statements ---

pub fn eval_block(block: &Block, env: &mut Env) -> EvalResult {
    env.push_scope();
    let result = eval_block_body(block, env);
    env.pop_scope();
    result
}

fn eval_block_body(block: &Block, env: &mut Env) -> EvalResult {
    for stmt in &block.stmts {
        exec_stmt(stmt, env)?;
    }
    match &block.tail {
        Some(tail) => eval_expr(tail, env),
        None => Ok(Value::Unit),
    }
}

fn exec_stmt(stmt: &Node<Stmt>, env: &mut Env) -> Result<(), RuntimeError> {
    match &stmt.kind {
        Stmt::Let {
            name,
            is_mut,
            value,
            ..
        } => {
            let v = eval_expr(value, env)?;
            env.define(name, v, *is_mut);
            Ok(())
        }
        Stmt::Assign { name, value } => {
            let v = eval_expr(value, env)?;
            match env.assign(name, v) {
                Ok(()) => Ok(()),
                Err(AssignError::Undefined) => {
                    err(format!("undefined variable `{name}`"), &stmt.span)
                }
                Err(AssignError::Immutable) => err(
                    format!("can't assign to `{name}` because it isn't `mut`"),
                    &stmt.span,
                ),
            }
        }
        Stmt::While { cond, body } => {
            loop {
                let c = expect_bool(eval_expr(cond, env)?, &cond.span)?;
                if !c {
                    break;
                }
                eval_block(&body.kind, env)?;
            }
            Ok(())
        }
        Stmt::Return(_) => err("`return` isn't supported yet", &stmt.span),
        Stmt::Expr(e) => {
            eval_expr(e, env)?;
            Ok(())
        }
    }
}

// --- expressions ---

pub fn eval_expr(expr: &Node<Expr>, env: &mut Env) -> EvalResult {
    match &expr.kind {
        Expr::Int(n) => Ok(Value::Int(*n)),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Str(s) => Ok(Value::Str(s.clone())),
        Expr::Ident(name) => match env.get(name) {
            Some(v) => Ok(v.clone()),
            None => err(format!("undefined variable `{name}`"), &expr.span),
        },
        Expr::Unary { op, operand } => {
            let v = eval_expr(operand, env)?;
            eval_unary(*op, v, &expr.span)
        }
        Expr::Binary { op, lhs, rhs } => eval_binary(*op, lhs, rhs, &expr.span, env),
        Expr::Block(block) => eval_block(block, env),
        Expr::If {
            cond,
            then_block,
            else_branch,
        } => {
            let c = expect_bool(eval_expr(cond, env)?, &cond.span)?;
            if c {
                eval_block(&then_block.kind, env)
            } else {
                match else_branch {
                    Some(e) => eval_expr(e, env),
                    None => Ok(Value::Unit),
                }
            }
        }
        Expr::Call { .. } => err("function calls aren't supported yet", &expr.span),
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

fn eval_binary(
    op: BinaryOp,
    lhs: &Node<Expr>,
    rhs: &Node<Expr>,
    span: &Span,
    env: &mut Env,
) -> EvalResult {
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        return eval_logic(op, lhs, rhs, env);
    }

    let l = eval_expr(lhs, env)?;
    let r = eval_expr(rhs, env)?;
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

fn eval_logic(op: BinaryOp, lhs: &Node<Expr>, rhs: &Node<Expr>, env: &mut Env) -> EvalResult {
    let l = expect_bool(eval_expr(lhs, env)?, &lhs.span)?;
    match (op, l) {
        (BinaryOp::And, false) => return Ok(Value::Bool(false)),
        (BinaryOp::Or, true) => return Ok(Value::Bool(true)),
        _ => {}
    }
    let r = expect_bool(eval_expr(rhs, env)?, &rhs.span)?;
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
        eval_block(&f.body.kind, &mut Env::new())
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

    #[test]
    fn let_and_use() {
        assert_eq!(run("let x = 5\n x * 2"), Ok(Value::Int(10)));
    }

    #[test]
    fn shadowing_sees_the_old_value() {
        assert_eq!(run("let x = 1\n let x = x + 1\n x"), Ok(Value::Int(2)));
    }

    #[test]
    fn assignment_needs_mut() {
        assert_eq!(run("let mut x = 1\n x = x + 4\n x"), Ok(Value::Int(5)));
        assert_eq!(
            msg("let x = 1\n x = 2\n x"),
            "can't assign to `x` because it isn't `mut`"
        );
    }

    #[test]
    fn undefined_variables() {
        assert_eq!(msg("y"), "undefined variable `y`");
        assert_eq!(msg("y = 1\n 0"), "undefined variable `y`");
    }

    #[test]
    fn blocks_have_their_own_scope() {
        assert_eq!(
            run("let x = 1\n let y = { let x = 2\n x + 10 }\n x + y"),
            Ok(Value::Int(13))
        );
        assert_eq!(run("let mut x = 1\n { x = 5 }\n x"), Ok(Value::Int(5)));
        assert_eq!(msg("{ let z = 1\n z }\n z"), "undefined variable `z`");
    }

    #[test]
    fn while_loops() {
        let src = "let mut i = 0\n let mut total = 0\n while i < 5 {\n total = total + i\n i = i + 1\n }\n total";
        assert_eq!(run(src), Ok(Value::Int(10)));
    }

    #[test]
    fn if_expressions() {
        assert_eq!(
            run("let x = 5\n if x > 3 { 1 } else { 2 }"),
            Ok(Value::Int(1))
        );
        assert_eq!(
            run("if false { 1 } else if true { 2 } else { 3 }"),
            Ok(Value::Int(2))
        );
        assert_eq!(run("if false { 1 }"), Ok(Value::Unit));
        assert_eq!(msg("if 1 { 2 }"), "expected a bool, got a int");
    }
}
