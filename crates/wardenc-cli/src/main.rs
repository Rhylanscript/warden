use std::process::ExitCode;
use wardenc_syntax::lexer::lex;
use wardenc_syntax::parser::parse;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    if args.len() != 3 || !matches!(args[1].as_str(), "lex" | "parse") {
        eprintln!("usage: warden <lex|parse> <file>");
        return ExitCode::FAILURE;
    }

    let src = match std::fs::read_to_string(&args[2]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: couldn't read {}: {e}", args[2]);
            return ExitCode::FAILURE;
        }
    };

    if args[1] == "lex" {
        run_lex(&src)
    } else {
        run_parse(&args[2], &src)
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
                let (line, col) = line_col(src, e.span.start);
                eprintln!("error: {}\n  --> {file}:{line}:{col}", e.message);
            }
            ExitCode::FAILURE
        }
    }
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
