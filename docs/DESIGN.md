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
