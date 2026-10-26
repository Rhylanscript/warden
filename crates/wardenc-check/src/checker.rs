use crate::effects::{EffectPath, covers, display, intrinsic_effect};
use std::collections::HashMap;
use wardenc_syntax::ast::*;

#[derive(Debug, Clone, PartialEq)]
pub struct CheckError {
    pub message: String,
    pub span: Span,
}

/// check every function in the program - unlike the parser we keep going after
/// the first problem so one go reports all bad calls
pub fn check(program: &Program) -> Result<(), Vec<CheckError>> {
    // first pass: what each function declares
    let mut table: HashMap<&str, Vec<EffectPath>> = HashMap::new();
    for item in &program.items {
        let Item::Fn(decl) = &item.kind;
        table.insert(decl.name.as_str(), declared_effects(decl));
    }

    // second pass: walk every body and check each call against its functions `uses`
    let mut errors = Vec::new();
    for item in &program.items {
        let Item::Fn(decl) = &item.kind;
        let mut fc = FnChecker {
            fn_name: &decl.name,
            allowed: declared_effects(decl),
            table: &table,
            errors: &mut errors,
        };
        fc.block(&decl.body.kind);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn declared_effects(decl: &FnDecl) -> Vec<EffectPath> {
    decl.effects.iter().map(|e| e.kind.path.clone()).collect()
}

/// Checks the body of one function
struct FnChecker<'a> {
    fn_name: &'a str,
    /// what this functions `uses` clause allows
    allowed: Vec<EffectPath>,
    /// what every function in the program declares
    table: &'a HashMap<&'a str, Vec<EffectPath>>,
    errors: &'a mut Vec<CheckError>,
}

impl FnChecker<'_> {
    fn block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.stmt(&stmt.kind);
        }
        if let Some(tail) = &block.tail {
            self.expr(tail);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { value, .. } | Stmt::Assign { value, .. } => self.expr(value),
            Stmt::While { cond, body } => {
                self.expr(cond);
                self.block(&body.kind);
            }
            Stmt::Return(value) => {
                if let Some(v) = value {
                    self.expr(v);
                }
            }
            Stmt::Expr(e) => self.expr(e),
        }
    }

    fn expr(&mut self, expr: &Node<Expr>) {
        match &expr.kind {
            Expr::Int(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Ident(_) => {}
            Expr::Unary { operand, .. } => self.expr(operand),
            Expr::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            Expr::Block(block) => self.block(block),
            Expr::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.expr(cond);
                self.block(&then_block.kind);
                if let Some(e) = else_branch {
                    self.expr(e);
                }
            }
            Expr::Call { callee, args } => {
                // arguments can contain calls too e.g. `f(print(1))`
                for arg in args {
                    self.expr(arg);
                }
                if let Expr::Ident(name) = &callee.kind {
                    self.check_call(name, &expr.span);
                }
            }
        }
    }

    /// The heart of the checker: this call needs some effects and the
    /// enclosing function has to allow every one of them
    fn check_call(&mut self, name: &str, span: &Span) {
        // same order as the interpreter; intrinsics first then user functions
        let required: Vec<EffectPath> = if let Some(effect) = intrinsic_effect(name) {
            vec![effect]
        } else if let Some(effects) = self.table.get(name) {
            effects.clone()
        } else {
            // unknown function: the interpreter reports that one (for now)
            return;
        };

        for effect in required {
            if !self.allowed.iter().any(|a| covers(a, &effect)) {
                let needed = display(&effect);
                self.errors.push(CheckError {
                    message: format!(
                        "calling `{name}` requires effect `{needed}`, but `{}` doesn't declare it (add `uses {needed}`)",
                        self.fn_name
                    ),
                    span: span.clone(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wardenc_syntax::parser::parse;

    fn errors(src: &str) -> Vec<String> {
        let program = parse(src).expect("should parse");
        match check(&program) {
            Ok(()) => Vec::new(),
            Err(errs) => errs.into_iter().map(|e| e.message).collect(),
        }
    }

    #[test]
    fn pure_functions_are_fine() {
        assert!(errors("fn add(a: int, b: int) -> int { a + b }").is_empty());
    }

    #[test]
    fn print_needs_io_print() {
        assert_eq!(
            errors("fn main() { print(1) }"),
            vec![
                "calling `print` requires effect `io.print`, but `main` doesn't declare it (add `uses io.print`)"
            ]
        );
    }

    #[test]
    fn declared_effect_is_enough() {
        assert!(errors("fn main() uses io.print { print(1) }").is_empty());
    }

    #[test]
    fn parent_effect_covers_child() {
        assert!(errors("fn main() uses io { print(1) }").is_empty());
    }

    #[test]
    fn unrelated_effect_does_not_cover() {
        assert_eq!(errors("fn main() uses fs.read { print(1) }").len(), 1);
    }

    #[test]
    fn child_effect_does_not_cover_parent() {
        assert!(!crate::effects::covers(
            &["io".to_string(), "print".to_string()],
            &["io".to_string()]
        ));
    }

    #[test]
    fn calling_an_effectful_function_needs_its_effects() {
        let src = "fn say() uses io.print { print(1) }\nfn main() { say() }";
        assert_eq!(
            errors(src),
            vec![
                "calling `say` requires effect `io.print`, but `main` doesn't declare it (add `uses io.print`)"
            ]
        );
    }

    #[test]
    fn calls_nested_in_control_flow_are_found() {
        assert_eq!(errors("fn main() { if true { print(1) } }").len(), 1);
        assert_eq!(
            errors("fn main() { let mut i = 0\n while i < 3 { print(i)\n i = i + 1 } }").len(),
            1
        );
    }

    #[test]
    fn every_bad_call_is_reported() {
        assert_eq!(errors("fn a() { print(1) }\nfn b() { print(2) }").len(), 2);
    }

    #[test]
    fn all_top_level_examples_pass() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
        let mut count = 0;
        for entry in std::fs::read_dir(dir).expect("examples dir should exist") {
            let path = entry.expect("readable entry").path();
            if path.extension().is_some_and(|e| e == "wd") {
                let src = std::fs::read_to_string(&path).expect("readable file");
                let program = parse(&src).expect("example should parse");
                let result = check(&program);
                assert!(result.is_ok(), "{} failed: {result:?}", path.display());
                count += 1;
            }
        }
        assert!(count > 0, "no examples found");
    }
}
