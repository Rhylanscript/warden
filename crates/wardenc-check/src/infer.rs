use crate::effects::{EffectSet, intrinsic_effect};
use std::collections::HashMap;
use wardenc_syntax::ast::*;

/// What every function in the program is allowed to do (by name)
pub type EffectTable<'a> = HashMap<&'a str, EffectSet>;

/// a call found in function body
pub struct CallSite<'a> {
    pub name: &'a str,
    pub span: Span,
}

/// Every call (by name) in a block including ones nested in `if`,
/// `while` and call args
pub fn calls_in(block: &Block) -> Vec<CallSite<'_>> {
    let mut calls = Vec::new();
    walk_block(block, &mut calls);
    calls
}

fn walk_block<'a>(block: &'a Block, out: &mut Vec<CallSite<'a>>) {
    for stmt in &block.stmts {
        walk_stmt(&stmt.kind, out);
    }
    if let Some(tail) = &block.tail {
        walk_expr(tail, out);
    }
}

fn walk_stmt<'a>(stmt: &'a Stmt, out: &mut Vec<CallSite<'a>>) {
    match stmt {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } => walk_expr(value, out),
        Stmt::While { cond, body } => {
            walk_expr(cond, out);
            walk_block(&body.kind, out);
        }
        Stmt::Return(value) => {
            if let Some(v) = value {
                walk_expr(v, out);
            }
        }
        Stmt::Expr(e) => walk_expr(e, out),
    }
}

fn walk_expr<'a>(expr: &'a Node<Expr>, out: &mut Vec<CallSite<'a>>) {
    match &expr.kind {
        Expr::Int(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Ident(_) => {}
        Expr::Unary { operand, .. } => walk_expr(operand, out),
        Expr::Binary { lhs, rhs, .. } => {
            walk_expr(lhs, out);
            walk_expr(rhs, out);
        }
        Expr::Block(block) => walk_block(block, out),
        Expr::If {
            cond,
            then_block,
            else_branch,
        } => {
            walk_expr(cond, out);
            walk_block(&then_block.kind, out);
            if let Some(e) = else_branch {
                walk_expr(e, out);
            }
        }
        Expr::Call { callee, args } => {
            // args can contain calls too
            for arg in args {
                walk_expr(arg, out);
            }
            if let Expr::Ident(name) = &callee.kind {
                out.push(CallSite {
                    name: name.as_str(),
                    span: expr.span.clone(),
                });
            }
        }
    }
}

/// What a function declares in it's `uses` statement
pub fn declared_set(decl: &FnDecl) -> EffectSet {
    EffectSet::from_paths(decl.effects.iter().map(|e| e.kind.path.clone()))
}

/// Fixed functions state their effects themselves: `pub` ones (the public
/// contract) and private ones with a `uses` clause (a ceiling), everything
/// else is inferred
pub fn is_fixed(decl: &FnDecl) -> bool {
    decl.is_pub || !decl.effects.is_empty()
}

/// The effects of calling `name`. Uses same order as the interpreter:
/// intrinsics first, then user functions. Unknown functions give an empty
/// set (the interpreter will report those for now)
pub fn effects_of_call(name: &str, table: &EffectTable<'_>) -> EffectSet {
    if let Some(effect) = intrinsic_effect(name) {
        EffectSet::from_paths([effect])
    } else {
        table.get(name).cloned().unwrap_or_default()
    }
}

/// Work out what every function can do
/// - `pub` functions, and private ones with a `uses` clause, are **fixed**:
///   what is declared is what callers will see
/// - Every other function is **inferred**:
///   it starts empty and grows to the union of everything it calls, repeated
///   until nothing changes (a fixpoint)
pub fn effective_effects(program: &Program) -> EffectTable<'_> {
    let mut table = EffectTable::new();
    let mut inferred = Vec::new();

    for item in &program.items {
        let Item::Fn(decl) = &item.kind;
        let name = decl.name.as_str();
        if is_fixed(decl) {
            table.insert(name, declared_set(decl));
        } else {
            table.insert(name, EffectSet::new());
            inferred.push((name, calls_in(&decl.body.kind)));
        }
    }

    loop {
        let mut changed = false;
        for (name, calls) in &inferred {
            let mut needed = EffectSet::new();
            for call in calls {
                needed.extend(&effects_of_call(call.name, &table));
            }
            if let Some(entry) = table.get_mut(*name) {
                changed |= entry.extend(&needed);
            }
        }
        if !changed {
            break;
        }
    }

    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use wardenc_syntax::parser::parse;

    fn effects_of(src: &str, name: &str) -> String {
        let program = parse(src).expect("should parse");
        let table = effective_effects(&program);
        table[name].render()
    }

    #[test]
    fn private_helper_is_inferred() {
        let src = "fn say() { print(1) }\nfn main() { say() }";
        assert_eq!(effects_of(src, "say"), "io.print");
        assert_eq!(effects_of(src, "main"), "io.print");
    }

    #[test]
    fn chain_is_inferred_across_several_rounds() {
        let src = "fn a() { b() }\nfn b() { c() }\nfn c() { print(1) }";
        assert_eq!(effects_of(src, "a"), "io.print");
        assert_eq!(effects_of(src, "b"), "io.print");
        assert_eq!(effects_of(src, "c"), "io.print");
    }

    #[test]
    fn mutual_recursion_terminates_and_shares_effects() {
        let src = "fn ping(n: int) { pong(n) }\nfn pong(n: int) { print(n)\n ping(n) }";
        assert_eq!(effects_of(src, "ping"), "io.print");
        assert_eq!(effects_of(src, "pong"), "io.print");
    }

    #[test]
    fn pure_recursion_stays_pure() {
        let src = "fn fib(n: int) -> int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }";
        assert_eq!(effects_of(src, "fib"), "");
    }

    #[test]
    fn pub_functions_are_never_inferred() {
        assert_eq!(effects_of("pub fn f() { print(1) }", "f"), "");
        assert_eq!(effects_of("pub fn f() uses io { print(1) }", "f"), "io");
    }

    #[test]
    fn explicit_uses_on_a_private_function_is_what_callers_see() {
        assert_eq!(effects_of("fn f() uses io { print(1) }", "f"), "io");
    }

    #[test]
    fn inferred_sets_are_normalised() {
        let src = "fn g() uses io { }\nfn f() { g()\n print(1) }";
        assert_eq!(effects_of(src, "f"), "io");
    }
}
