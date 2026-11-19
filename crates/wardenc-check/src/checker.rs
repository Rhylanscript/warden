use crate::effects::display;
use crate::infer::{
    EffectTable, calls_in, declared_set, effective_effects, effects_of_call, is_fixed,
};
use wardenc_syntax::ast::*;

#[derive(Debug, Clone, PartialEq)]
pub struct CheckError {
    pub message: String,
    pub span: Span,
}

/// check every function in the program - unlike the parser we keep going after
/// the first problem so one go reports all bad calls
pub fn check(program: &Program) -> Result<(), Vec<CheckError>> {
    let table = effective_effects(program);

    let mut errors = Vec::new();
    for item in &program.items {
        let Item::Fn(decl) = &item.kind;
        if is_fixed(decl) {
            check_fn(decl, &table, &mut errors);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Check one fixed function: every call in its body must be allowed by what
/// the function declares
fn check_fn(decl: &FnDecl, table: &EffectTable<'_>, errors: &mut Vec<CheckError>) {
    let allowed = declared_set(decl);
    let calls = calls_in(&decl.body.kind);
    let mut suggested = allowed.clone();
    for call in &calls {
        suggested.extend(&effects_of_call(call.name, table));
    }

    for call in &calls {
        let required = effects_of_call(call.name, table);
        for effect in required.iter() {
            if !allowed.allows(effect) {
                errors.push(CheckError {
                    message: format!(
                        "calling `{}` requires effect `{}`, but `{}` doesn't declare it (add `uses {}`)",
                        call.name,
                        display(effect),
                        decl.name,
                        suggested.render()
                    ),
                    span: call.span.clone(),
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
            errors("pub fn main() { print(1) }"),
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
        let src = "fn say() uses io.print { print(1) }\npub fn main() { say() }";
        assert_eq!(
            errors(src),
            vec![
                "calling `say` requires effect `io.print`, but `main` doesn't declare it (add `uses io.print`)"
            ]
        );
    }

    #[test]
    fn calls_nested_in_control_flow_are_found() {
        assert_eq!(errors("pub fn main() { if true { print(1) } }").len(), 1);
        assert_eq!(
            errors("pub fn main() { let mut i = 0\n while i < 3 { print(i)\n i = i + 1 } }").len(),
            1
        );
    }

    #[test]
    fn every_bad_call_is_reported() {
        assert_eq!(
            errors("pub fn a() { print(1) }\npub fn b() { print(2) }").len(),
            2
        );
    }

    #[test]
    fn private_helpers_need_no_uses() {
        let src = "fn helper() { print(1) }\nfn main() uses io.print { helper() }";
        assert!(errors(src).is_empty());
    }

    #[test]
    fn inferred_effects_reach_a_pub_function_through_a_chain() {
        let src = "fn a() { b() }\nfn b() { print(1) }\npub fn main() { a() }";
        assert_eq!(
            errors(src),
            vec![
                "calling `a` requires effect `io.print`, but `main` doesn't declare it (add `uses io.print`)"
            ]
        );
    }

    #[test]
    fn mutual_recursion_is_fine_when_the_caller_declares_it() {
        let src = "fn ping() { pong() }\nfn pong() { print(1)\n ping() }\nfn main() uses io.print { ping() }";
        assert!(errors(src).is_empty());
    }

    #[test]
    fn explicit_uses_on_a_private_function_is_a_ceiling() {
        assert_eq!(errors("fn f() uses fs.read { print(1) }").len(), 1);
    }

    #[test]
    fn declaring_more_than_needed_is_allowed() {
        assert!(errors("pub fn f() uses io.print, fs.read { print(1) }").is_empty());
    }

    #[test]
    fn suggestion_is_the_complete_uses_clause() {
        assert_eq!(
            errors("pub fn f() uses fs.read { print(1) }"),
            vec![
                "calling `print` requires effect `io.print`, but `f` doesn't declare it (add `uses fs.read, io.print`)"
            ]
        );
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
