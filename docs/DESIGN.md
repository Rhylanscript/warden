# Warden design log

Newest decisions go at the bottom. Each entry: says what and why.

## Generics use `[T]` instead of `<T>`

`<` is also less than, which makes parsing annoying. `[T]` keeps the grammar simple. Indexing will use `xs[i]`, which we can tell apart by position.

## Statements end with newline or `;`

Similar to go and kotlin. The lexer emits a newline token and the parser ignores it in "unfinished" positions (after `+`, `,`, `(` and so on)

## `print` is an intrinsic rather than a keyword

Effects originate only from stdlib intrinsics. `print` is a compiler known function tagged `io.print`.

## `main` may return `Result[(), E]`

This lets `?` work in `main`. The runtime prints the error and exits nonzero

## Struct literals vs blocks in `if` conditions

Paused until v1. Rust restricts struct literals in condition position. Warden will probably do the same.

## The interpreter lives in its own crate

`wardenc-interp` depends on `wardenc-syntax` but not the other way round, so the syntax crate stays reusable for the checker and future backends and stuff.

## `return` unwinds through a `Control` enum

`return` isnt an error but it unwinds the same way, so eval functions return `Result<T, Control>` and `?` carries both. `call_function` catches `Control::Return`.

## Recursion is capped at 1000 calls

Warden recursion is Rust recursion. The CLI runs the interpreter on a 64MB stack thread so the cap is hit before a stack overflow.

## Output goes through an injected writer

`run_program` takes `&mut dyn Write` so tests can capture output instead of printing it.

## Effects are checked statically in their own crate

`wardenc-check` walks the AST & never runs anything. A parent effect covers its children (`io` covers `io.print`, not the other way round) The checker is separate from the interpreter and only the CLI wires them together

## Private functions get their effects inferred

Writing `uses` on every little helper is annoying. private functions with no `uses` clause get the union of everything they call. `pub` functions and `main` never infer, what they declare is the contract, and the body is checked by the compiler.

## Inference is a fixpoint over the call graph

Functions can call each other (`fib` calls itself), so there is no bottom to recurse to. Every inferred function starts at the empty set and gets recomputed from its callees current sets until a whole round changes nothing. It terminates because sets only grow and the number of effects in a program is finite.

## An explicit `uses` on a private function is a ceiling

If you write `uses` on a private function the compiler holds you to it: the body cant exceed it, and callers see exactly what you declared. Declaring more than the body needs is fine.

## `main` is always fixed

`main` is the entry point, which means its `uses` clause is the whole programs capability list. That is what `warden audit` will report later, so it has to be shown even though `main` is private.

## Effect sets are normalised

An effect set never holds a parent and one of its children, so `{io, io.print}` collapses to `{io}`. This keeps error messages and suggested `uses` clauses tidy. The suggestion in an error is always the complete clause to paste in rather than just the one missing effect.
