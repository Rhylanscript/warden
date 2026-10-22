use std::process::ExitCode;
use wardenc_syntax::lexer::lex;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    if args.len() != 3 || args[1] != "lex" {
        eprintln!("usage: warden lex <file>");
        return ExitCode::FAILURE;
    }

    let src = match std::fs::read_to_string(&args[2]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: couldn't read {}: {e}", args[2]);
            return ExitCode::FAILURE;
        }
    };

    let (tokens, errors) = lex(&src);

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
