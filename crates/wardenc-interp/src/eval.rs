use crate::env::{AssignError, Env};
use crate::value::{RuntimeError, Value};
use std::collections::HashMap;
use std::io::Write;
use wardenc_syntax::ast::*;

pub type EvalResult = Result<Value, RuntimeError>;

/// Why evaluation stopped early; `return` isn't an error, but it unwinds
/// the same way, so both travel up through `?`
enum Control {
    Return(Value),
    Error(RuntimeError),
}

impl From<RuntimeError> for Control {
    fn from(e: RuntimeError) -> Self {
        Control::Error(e)
    }
}

type Flow<T> = Result<T, Control>;

/// Warden recursion is Rust recursion, so we cap it at a reasonable depth to avoid blowing the stack
const MAX_DEPTH: usize = 1000;

fn err<T, E: From<RuntimeError>>(message: impl Into<String>, span: &Span) -> Result<T, E> {
    Err(RuntimeError {
        message: message.into(),
        span: span.clone(),
    }
    .into())
}

fn overflow(span: &Span) -> RuntimeError {
    RuntimeError {
        message: "integer overflow".to_string(),
        span: span.clone(),
    }
}

/// Run a whole program: find `main` and call it
pub fn run_program(program: &Program, out: &mut dyn Write) -> Result<(), RuntimeError> {
    Interp::new(program, out)?.run_main().map(|_| ())
}

pub struct Interp<'p, 'o> {
    functions: HashMap<&'p str, &'p FnDecl>,
    out: &'o mut dyn Write,
    depth: usize,
}

impl<'p, 'o> Interp<'p, 'o> {
    /// builds the function table from the programs items.
    pub fn new(program: &'p Program, out: &'o mut dyn Write) -> Result<Self, RuntimeError> {
        let mut functions = HashMap::new();
        for item in &program.items {
            let Item::Fn(decl) = &item.kind;
            if functions.insert(decl.name.as_str(), decl).is_some() {
                return err(
                    format!("function `{}` is defined twice", decl.name),
                    &item.span,
                );
            }
        }
        Ok(Interp {
            functions,
            out,
            depth: 0,
        })
    }

    pub fn run_main(&mut self) -> EvalResult {
        let Some(main) = self.functions.get("main").copied() else {
            return err("no `main` function found", &(0..0));
        };
        if !main.params.is_empty() {
            return err("`main` can't take parameters", &main.body.span);
        }
        self.call_function(main, Vec::new())
    }

    /// call a user defined function with already evaluated arguments
    fn call_function(&mut self, decl: &FnDecl, args: Vec<Value>) -> EvalResult {
        if self.depth >= MAX_DEPTH {
            return err("stack overflow: too many nested calls", &decl.body.span);
        }
        // a brand new env: the callee cannot see the callers variables
        let mut env = Env::new();
        for (param, value) in decl.params.iter().zip(args) {
            env.define(&param.name, value, false);
        }
        self.depth += 1;
        let result = self.eval_block(&decl.body.kind, &mut env);
        self.depth -= 1;
        match result {
            // either the body finished normally or a `return` bubbled up to here
            Ok(v) | Err(Control::Return(v)) => Ok(v),
            Err(Control::Error(e)) => Err(e),
        }
    }

    fn eval_call(
        &mut self,
        callee: &Node<Expr>,
        args: &[Node<Expr>],
        span: &Span,
        env: &mut Env,
    ) -> Flow<Value> {
        let Expr::Ident(name) = &callee.kind else {
            return err("only named functions can be called", &callee.span);
        };
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval_expr(arg, env)?);
        }
        if name.as_str() == "print" {
            return self.call_print(&values, span);
        }
        let Some(decl) = self.functions.get(name.as_str()).copied() else {
            return err(format!("undefined function `{name}`"), &callee.span);
        };
        if decl.params.len() != values.len() {
            return err(
                format!(
                    "`{name}` takes {} argument(s), but {} were given",
                    decl.params.len(),
                    values.len()
                ),
                span,
            );
        }
        self.call_function(decl, values).map_err(Control::Error)
    }

    /// The `print` intrinsic (tagged `io.print` once the effect checker exists)
    fn call_print(&mut self, args: &[Value], span: &Span) -> Flow<Value> {
        let [value] = args else {
            return err(
                format!("`print` takes 1 argument, but {} were given", args.len()),
                span,
            );
        };
        writeln!(self.out, "{value}").map_err(|e| RuntimeError {
            message: format!("couldn't write output: {e}"),
            span: span.clone(),
        })?;
        Ok(Value::Unit)
    }

    // --- blocks & statements ---

    /// Run a block in its own scope, its value is the tail expression, or Unit
    fn eval_block(&mut self, block: &Block, env: &mut Env) -> Flow<Value> {
        env.push_scope();
        let result = self.eval_block_body(block, env);
        // pop even if something failed or returned so the scope stack stays balanced
        env.pop_scope();
        result
    }

    fn eval_block_body(&mut self, block: &Block, env: &mut Env) -> Flow<Value> {
        for stmt in &block.stmts {
            self.exec_stmt(stmt, env)?;
        }
        match &block.tail {
            Some(tail) => self.eval_expr(tail, env),
            None => Ok(Value::Unit),
        }
    }

    fn exec_stmt(&mut self, stmt: &Node<Stmt>, env: &mut Env) -> Flow<()> {
        match &stmt.kind {
            Stmt::Let {
                name,
                is_mut,
                value,
                ..
            } => {
                // eval first define after so `let x = x + 1` sees the old x
                let v = self.eval_expr(value, env)?;
                env.define(name, v, *is_mut);
                Ok(())
            }
            Stmt::Assign { name, value } => {
                let v = self.eval_expr(value, env)?;
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
                    let c = expect_bool(self.eval_expr(cond, env)?, &cond.span)?;
                    if !c {
                        break;
                    }
                    self.eval_block(&body.kind, env)?;
                }
                Ok(())
            }
            Stmt::Return(value) => {
                let v = match value {
                    Some(e) => self.eval_expr(e, env)?,
                    None => Value::Unit,
                };
                Err(Control::Return(v))
            }
            Stmt::Expr(e) => {
                self.eval_expr(e, env)?;
                Ok(())
            }
        }
    }

    // --- expressions ---

    fn eval_expr(&mut self, expr: &Node<Expr>, env: &mut Env) -> Flow<Value> {
        match &expr.kind {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Ident(name) => match env.get(name) {
                Some(v) => Ok(v.clone()),
                None => err(format!("undefined variable `{name}`"), &expr.span),
            },
            Expr::Unary { op, operand } => {
                let v = self.eval_expr(operand, env)?;
                eval_unary(*op, v, &expr.span).map_err(Control::Error)
            }
            Expr::Binary { op, lhs, rhs } => self.eval_binary(*op, lhs, rhs, &expr.span, env),
            Expr::Block(block) => self.eval_block(block, env),
            Expr::If {
                cond,
                then_block,
                else_branch,
            } => {
                let c = expect_bool(self.eval_expr(cond, env)?, &cond.span)?;
                if c {
                    self.eval_block(&then_block.kind, env)
                } else {
                    match else_branch {
                        Some(e) => self.eval_expr(e, env),
                        None => Ok(Value::Unit),
                    }
                }
            }
            Expr::Call { callee, args } => self.eval_call(callee, args, &expr.span, env),
        }
    }

    fn eval_binary(
        &mut self,
        op: BinaryOp,
        lhs: &Node<Expr>,
        rhs: &Node<Expr>,
        span: &Span,
        env: &mut Env,
    ) -> Flow<Value> {
        // `&&` and `||` are special: the right side might not run at all
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return self.eval_logic(op, lhs, rhs, env);
        }

        let l = self.eval_expr(lhs, env)?;
        let r = self.eval_expr(rhs, env)?;
        let result = match (l, r) {
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
        };
        result.map_err(Control::Error)
    }

    fn eval_logic(
        &mut self,
        op: BinaryOp,
        lhs: &Node<Expr>,
        rhs: &Node<Expr>,
        env: &mut Env,
    ) -> Flow<Value> {
        let l = expect_bool(self.eval_expr(lhs, env)?, &lhs.span)?;
        // short circuit: `false && _` is false, `true || _` is true
        match (op, l) {
            (BinaryOp::And, false) => return Ok(Value::Bool(false)),
            (BinaryOp::Or, true) => return Ok(Value::Bool(true)),
            _ => {}
        }
        // otherwise the answer is just whatever the right side is
        let r = expect_bool(self.eval_expr(rhs, env)?, &rhs.span)?;
        Ok(Value::Bool(r))
    }
}

// --- helpers that dont need interpreter state ---

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
        BinaryOp::And | BinaryOp::Or => unreachable!("handled in eval_logic"),
    }
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
        let mut sink = Vec::new();
        Interp::new(&program, &mut sink)?.run_main()
    }

    fn msg(src: &str) -> String {
        run(src).expect_err("should fail").message
    }

    fn output(src: &str) -> Result<String, RuntimeError> {
        let program = parse(src).expect("should parse");
        let mut buf = Vec::new();
        run_program(&program, &mut buf)?;
        Ok(String::from_utf8(buf).expect("output should be utf-8"))
    }

    fn out_msg(src: &str) -> String {
        output(src).expect_err("should fail").message
    }

    // ---- expressions ----

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

    // ---- vars, blocks, loops, if ----

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

    // ---- funcs, return, print ----

    #[test]
    fn hello_example() {
        let src = include_str!("../../../examples/hello.wd");
        assert_eq!(output(src), Ok("hello, warden\n".to_string()));
    }

    #[test]
    fn fib_example() {
        let src = include_str!("../../../examples/fib.wd");
        assert_eq!(output(src), Ok("6765\n".to_string()));
    }

    #[test]
    fn loops_example() {
        let src = include_str!("../../../examples/loops.wd");
        assert_eq!(output(src), Ok("5050\n".to_string()));
    }

    #[test]
    fn precedence_example() {
        let src = include_str!("../../../examples/precedence.wd");
        assert_eq!(output(src), Ok("7\n9\n3\ntrue\n".to_string()));
    }

    #[test]
    fn return_unwinds_out_of_loops() {
        let src = r#"
fn first_square_over(limit: int) -> int {
    let mut i = 0
    while true {
        if i * i > limit {
            return i
        }
        i = i + 1
    }
    0
}

fn main() uses io.print {
    print(first_square_over(50))
}
"#;
        assert_eq!(output(src), Ok("8\n".to_string()));
    }

    #[test]
    fn bare_return_gives_unit() {
        let src = "fn f() { return }\nfn main() uses io.print { print(f()) }";
        assert_eq!(output(src), Ok("()\n".to_string()));
    }

    #[test]
    fn callee_cannot_see_callers_variables() {
        let src = r#"
fn peek() -> int {
    x
}

fn main() uses io.print {
    let x = 1
    print(peek())
}
"#;
        assert_eq!(out_msg(src), "undefined variable `x`");
    }

    #[test]
    fn call_errors() {
        assert_eq!(
            out_msg("fn f(a: int) -> int { a }\nfn main() { f(1, 2) }"),
            "`f` takes 1 argument(s), but 2 were given"
        );
        assert_eq!(out_msg("fn main() { nope() }"), "undefined function `nope`");
        assert_eq!(
            out_msg("fn main() { print(1, 2) }"),
            "`print` takes 1 argument, but 2 were given"
        );
    }

    #[test]
    fn program_structure_errors() {
        assert_eq!(out_msg("fn f() { 1 }"), "no `main` function found");
        assert_eq!(
            out_msg("fn main() { 1 }\nfn main() { 2 }"),
            "function `main` is defined twice"
        );
        assert_eq!(
            out_msg("fn main(x: int) { 1 }"),
            "`main` can't take parameters"
        );
    }

    #[test]
    fn runaway_recursion_is_an_error() {
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| output("fn f() -> int { f() }\nfn main() { f() }").map_err(|e| e.message))
            .expect("thread should spawn");
        assert_eq!(
            handle.join().expect("thread shouldn't panic"),
            Err("stack overflow: too many nested calls".to_string())
        );
    }
}
