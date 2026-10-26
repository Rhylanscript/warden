use std::ops::Range;
use std::process::ExitCode;
use wardenc_check::check;
use wardenc_interp::eval::run_program;
use wardenc_syntax::ast::Program;
use wardenc_syntax::lexer::lex;
use wardenc_syntax::parser::parse;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    if args.len() != 3 || !matches!(args[1].as_str(), "lex" | "parse" | "run" | "check") {
        eprintln!("usage: warden <lex|parse|run|check> <file>");
        return ExitCode::FAILURE;
    }

    let src = match std::fs::read_to_string(&args[2]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: couldn't read {}: {e}", args[2]);
            return ExitCode::FAILURE;
        }
    };

    match args[1].as_str() {
        "lex" => run_lex(&src),
        "parse" => run_parse(&args[2], &src),
        "check" => run_check(&args[2], &src),
        _ => run_file(&args[2], &src),
    }
}

fn run_lex(src: &str) -> ExitCode {
    let (tokens, errors) = lex(src);
    for t in &tokens {
        println!("{:?} {:?} {:?}", t.token, t.span, &src[t.span.clone()]);
    }
    for span in &errors {
        eprintln!(
            "error: unexpected character {:?} at {:?}",
            &src[span.clone()],
            span
        );
    }
    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_parse(file: &str, src: &str) -> ExitCode {
    match parse(src) {
        Ok(program) => {
            println!("{program:#?}");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for e in errors {
                report(file, src, &e.message, &e.span);
            }
            ExitCode::FAILURE
        }
    }
}

fn run_file(file: &str, src: &str) -> ExitCode {
    let Some(program) = parse_and_check(file, src) else {
        return ExitCode::FAILURE;
    };

    // Deep recursion in Warden is deep recursion in Rust, so run the
    // interpreter on a thread with a big stack
    let result = std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn_scoped(s, || {
                let stdout = std::io::stdout();
                let mut out = stdout.lock();
                run_program(&program, &mut out)
            })
            .expect("couldn't start the interpreter thread")
            .join()
            .expect("the interpreter thread panicked")
    });

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            report(file, src, &e.message, &e.span);
            ExitCode::FAILURE
        }
    }
}

/// parse then effect check; prints every error and returns None if anything failed
fn parse_and_check(file: &str, src: &str) -> Option<Program> {
    let program = match parse(src) {
        Ok(p) => p,
        Err(errors) => {
            for e in errors {
                report(file, src, &e.message, &e.span);
            }
            return None;
        }
    };
    if let Err(errors) = check(&program) {
        for e in errors {
            report(file, src, &e.message, &e.span);
        }
        return None;
    }
    Some(program)
}

fn run_check(file: &str, src: &str) -> ExitCode {
    match parse_and_check(file, src) {
        Some(_) => {
            println!("ok");
            ExitCode::SUCCESS
        }
        None => ExitCode::FAILURE,
    }
}

fn report(file: &str, src: &str, message: &str, span: &Range<usize>) {
    let (line, col) = line_col(src, span.start);
    eprintln!("error: {message}\n  --> {file}:{line}:{col}");
}

fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let before = &src[..offset.min(src.len())];
    let line = before.matches('\n').count() + 1;
    let col = match before.rfind('\n') {
        Some(i) => before.len() - i,
        None => before.len() + 1,
    };
    (line, col)
}
