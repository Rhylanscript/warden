use logos::Logos;
use std::ops::Range;

/// Every kind of token in Warden
#[derive(Logos, Debug, Clone, PartialEq, Eq)]
#[logos(skip r"[ \t\r]+")]
#[logos(skip r"//[^\n]*")]
pub enum Token {
    // ---- keywords ----
    #[token("fn")]
    Fn,
    #[token("pub")]
    Pub,
    #[token("uses")]
    Uses,
    #[token("let")]
    Let,
    #[token("mut")]
    Mut,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("while")]
    While,
    #[token("return")]
    Return,
    #[token("true")]
    True,
    #[token("false")]
    False,

    // ---- literals & names ----
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    Ident,
    #[regex(r"[0-9][0-9_]*")]
    Int,
    #[regex(r#""[^"]*""#)]
    Str,

    // ---- punctuation ----
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(";")]
    Semi,
    #[token(".")]
    Dot,
    #[token("->")]
    Arrow,

    // ---- operators ----
    #[token("=")]
    Assign,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<")]
    Lt,
    #[token("<=")]
    LtEq,
    #[token(">")]
    Gt,
    #[token(">=")]
    GtEq,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("!")]
    Bang,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,

    // a newline plus any blank lines / indentation after it
    // collapses into one newline token
    #[regex(r"\n[ \t\r\n]*")]
    Newline,
}

/// A token plus where it came from in the source (byte offsets).
/// Spans are what let us point at the exact spot in error messages later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned {
    pub token: Token,
    pub span: Range<usize>,
}

/// Lex a whole source string.
/// Returns the tokens and the spans of any characters we didn't recognise.
pub fn lex(src: &str) -> (Vec<Spanned>, Vec<Range<usize>>) {
    let mut tokens = Vec::new();
    let mut errors = Vec::new();

    for (result, span) in Token::lexer(src).spanned() {
        match result {
            Ok(token) => tokens.push(Spanned { token, span }),
            Err(()) => errors.push(span),
        }
    }

    (tokens, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Token> {
        let (tokens, errors) = lex(src);
        assert!(errors.is_empty(), "unexpected lex errors: {errors:?}");
        tokens.into_iter().map(|t| t.token).collect()
    }

    #[test]
    fn keywords_vs_identifiers() {
        assert_eq!(kinds("fn fnord"), vec![Token::Fn, Token::Ident]);
    }

    #[test]
    fn let_statement() {
        assert_eq!(
            kinds("let mut x = 5 + 3"),
            vec![
                Token::Let,
                Token::Mut,
                Token::Ident,
                Token::Assign,
                Token::Int,
                Token::Plus,
                Token::Int,
            ]
        );
    }

    #[test]
    fn comparison_operators_are_greedy() {
        assert_eq!(
            kinds("<= < == = ->"),
            vec![
                Token::LtEq,
                Token::Lt,
                Token::EqEq,
                Token::Assign,
                Token::Arrow
            ]
        );
    }

    #[test]
    fn comments_are_skipped_but_newlines_kept() {
        assert_eq!(
            kinds("a // hi\nb"),
            vec![Token::Ident, Token::Newline, Token::Ident]
        );
    }

    #[test]
    fn blank_lines_collapse() {
        assert_eq!(
            kinds("a\n\n   \nb"),
            vec![Token::Ident, Token::Newline, Token::Ident]
        );
    }

    #[test]
    fn spans_point_at_source() {
        let (tokens, _) = lex("let abc");
        assert_eq!(&"let abc"[tokens[1].span.clone()], "abc");
    }

    #[test]
    fn unknown_characters_are_reported() {
        let (_, errors) = lex("let @ x");
        assert_eq!(errors, vec![4..5]);
    }

    #[test]
    fn hello_world() {
        let src = "fn main() uses io.print {\n    print(\"hello, warden\")\n}\n";
        let ks = kinds(src);
        assert_eq!(ks[0], Token::Fn);
        assert!(ks.contains(&Token::Uses));
        assert!(ks.contains(&Token::Str));
    }
}
