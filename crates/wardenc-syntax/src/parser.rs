use crate::ast::*;
use crate::lexer::{Spanned, Token, lex};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

type PResult<T> = Result<T, ParseError>;

/// Lex & parse a whole source file
pub fn parse(src: &str) -> Result<Program, Vec<ParseError>> {
    let (tokens, lex_errors) = lex(src);
    if !lex_errors.is_empty() {
        return Err(lex_errors
            .into_iter()
            .map(|span| ParseError {
                message: "unexpected character".to_string(),
                span,
            })
            .collect());
    }
    let mut parser = Parser {
        src,
        tokens,
        pos: 0,
    };
    parser.parse_program().map_err(|e| vec![e])
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Spanned>,
    pos: usize,
}

/// What a statement position turned out to be
enum Parsed {
    Stmt(Node<Stmt>),
    Tail(Node<Expr>),
}

/// binding power of prefix opers (`-x`, `!x`). Tighter than any binary op.
const PREFIX_BP: u8 = 13;

fn binary_op(tok: &Token) -> Option<BinaryOp> {
    Some(match tok {
        Token::OrOr => BinaryOp::Or,
        Token::AndAnd => BinaryOp::And,
        Token::EqEq => BinaryOp::Eq,
        Token::NotEq => BinaryOp::NotEq,
        Token::Lt => BinaryOp::Lt,
        Token::LtEq => BinaryOp::LtEq,
        Token::Gt => BinaryOp::Gt,
        Token::GtEq => BinaryOp::GtEq,
        Token::Plus => BinaryOp::Add,
        Token::Minus => BinaryOp::Sub,
        Token::Star => BinaryOp::Mul,
        Token::Slash => BinaryOp::Div,
        Token::Percent => BinaryOp::Rem,
        _ => return None,
    })
}

/// (left, right) binding powers. Right = left + 1 makes the opr left associative
fn binding_power(op: BinaryOp) -> (u8, u8) {
    match op {
        BinaryOp::Or => (1, 2),
        BinaryOp::And => (3, 4),
        BinaryOp::Eq | BinaryOp::NotEq => (5, 6),
        BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => (7, 8),
        BinaryOp::Add | BinaryOp::Sub => (9, 10),
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => (11, 12),
    }
}

impl<'a> Parser<'a> {
    // --- token helpers ---
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|t| &t.token)
    }

    fn peek_at(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.pos + n).map(|t| &t.token)
    }

    fn at(&self, tok: &Token) -> bool {
        self.peek() == Some(tok)
    }

    fn eat(&mut self, tok: &Token) -> bool {
        if self.at(tok) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn text(&self, span: &Span) -> &'a str {
        let src: &'a str = self.src;
        &src[span.clone()]
    }

    fn current_span(&self) -> Span {
        match self.tokens.get(self.pos) {
            Some(t) => t.span.clone(),
            None => self.src.len()..self.src.len(),
        }
    }

    /// Where the most recently consumed token ended
    fn prev_end(&self) -> usize {
        if self.pos == 0 {
            0
        } else {
            self.tokens[self.pos - 1].span.end
        }
    }

    fn describe(&self) -> String {
        match self.peek() {
            None => "end of file".to_string(),
            Some(Token::Newline) => "end of line".to_string(),
            Some(_) => format!("`{}`", self.text(&self.current_span())),
        }
    }

    fn error<T>(&self, message: impl Into<String>) -> PResult<T> {
        Err(ParseError {
            message: message.into(),
            span: self.current_span(),
        })
    }

    fn expect(&mut self, tok: Token, what: &str) -> PResult<Span> {
        if self.at(&tok) {
            let span = self.current_span();
            self.pos += 1;
            Ok(span)
        } else {
            self.error(format!("expected {what}, got {}", self.describe()))
        }
    }

    fn expect_ident(&mut self, what: &str) -> PResult<(String, Span)> {
        let span = self.expect(Token::Ident, what)?;
        Ok((self.text(&span).to_string(), span))
    }

    fn skip_newlines(&mut self) {
        while self.at(&Token::Newline) {
            self.pos += 1;
        }
    }

    fn skip_terminators(&mut self) {
        while matches!(self.peek(), Some(Token::Newline | Token::Semi)) {
            self.pos += 1;
        }
    }

    // --- items ---

    fn parse_program(&mut self) -> PResult<Program> {
        let mut items = Vec::new();
        self.skip_terminators();
        while self.peek().is_some() {
            items.push(self.parse_item()?);
            self.skip_terminators();
        }
        Ok(Program { items })
    }

    fn parse_item(&mut self) -> PResult<Node<Item>> {
        let start = self.current_span().start;
        let decl = self.parse_fn_decl()?;
        Ok(Node {
            kind: Item::Fn(decl),
            span: start..self.prev_end(),
        })
    }

    fn parse_fn_decl(&mut self) -> PResult<FnDecl> {
        let is_pub = self.eat(&Token::Pub);
        self.expect(Token::Fn, "`fn`")?;
        let (name, _) = self.expect_ident("a function name")?;
        self.expect(Token::LParen, "`(`")?;
        let params = self.parse_params()?;
        let ret = if self.eat(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let effects = if self.eat(&Token::Uses) {
            self.parse_effects()?
        } else {
            Vec::new()
        };
        let body = self.parse_block()?;
        Ok(FnDecl {
            is_pub,
            name,
            params,
            ret,
            effects,
            body,
        })
    }

    /// Called right after `(`. Consumes up to and including `)`.
    fn parse_params(&mut self) -> PResult<Vec<Param>> {
        let mut params = Vec::new();
        self.skip_newlines();
        while !self.at(&Token::RParen) {
            let (name, _) = self.expect_ident("a parameter name")?;
            self.expect(Token::Colon, "`:`")?;
            let ty = self.parse_type()?;
            params.push(Param { name, ty });
            self.skip_newlines();
            if !self.eat(&Token::Comma) {
                break;
            }
            self.skip_newlines();
        }
        self.expect(Token::RParen, "`)`")?;
        Ok(params)
    }

    fn parse_type(&mut self) -> PResult<Node<TypeExpr>> {
        let (name, span) = self.expect_ident("a type name")?;
        Ok(Node {
            kind: TypeExpr::Named(name),
            span,
        })
    }

    /// `io.print, fs.read` (called right after `uses`)
    fn parse_effects(&mut self) -> PResult<Vec<Node<Effect>>> {
        let mut effects = Vec::new();
        loop {
            let (first, span) = self.expect_ident("an effect name")?;
            let start = span.start;
            let mut path = vec![first];
            while self.eat(&Token::Dot) {
                let (segment, _) = self.expect_ident("an effect name after `.`")?;
                path.push(segment);
            }
            effects.push(Node {
                kind: Effect { path },
                span: start..self.prev_end(),
            });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(effects)
    }

    // ===== blocks and statements =====

    fn parse_block(&mut self) -> PResult<Node<Block>> {
        let start = self.expect(Token::LBrace, "`{`")?.start;
        let mut stmts = Vec::new();
        let mut tail = None;
        self.skip_terminators();
        while !self.at(&Token::RBrace) {
            if self.peek().is_none() {
                return self.error("this block is never closed, expected `}`");
            }
            match self.parse_stmt()? {
                Parsed::Stmt(s) => stmts.push(s),
                Parsed::Tail(e) => {
                    tail = Some(Box::new(e));
                    break;
                }
            }
        }
        self.expect(Token::RBrace, "`}`")?;
        Ok(Node {
            kind: Block { stmts, tail },
            span: start..self.prev_end(),
        })
    }

    /// Consumes whatever ends a statement: a newline, a `;`, or nothing if `}` is next.
    /// Returns true if it was an explicit `;`.
    fn end_stmt(&mut self) -> PResult<bool> {
        match self.peek() {
            Some(Token::Semi) => {
                self.skip_terminators();
                Ok(true)
            }
            Some(Token::Newline) => {
                self.skip_terminators();
                Ok(false)
            }
            Some(Token::RBrace) => Ok(false),
            _ => self.error(format!(
                "expected a newline, `;` or `}}` after this, found {}",
                self.describe()
            )),
        }
    }

    fn parse_stmt(&mut self) -> PResult<Parsed> {
        let start = self.current_span().start;
        let kind = match self.peek().cloned() {
            Some(Token::Let) => self.parse_let()?,
            Some(Token::While) => self.parse_while()?,
            Some(Token::Return) => self.parse_return()?,
            Some(Token::Ident) if self.peek_at(1) == Some(&Token::Assign) => self.parse_assign()?,
            _ => {
                let expr = self.parse_expr()?;
                let end = self.prev_end();
                let had_semi = self.end_stmt()?;
                // An expression right before `}` (and not ended by `;`) is the block's value.
                if !had_semi && self.at(&Token::RBrace) {
                    return Ok(Parsed::Tail(expr));
                }
                return Ok(Parsed::Stmt(Node {
                    kind: Stmt::Expr(expr),
                    span: start..end,
                }));
            }
        };
        let end = self.prev_end();
        self.end_stmt()?;
        Ok(Parsed::Stmt(Node {
            kind,
            span: start..end,
        }))
    }

    fn parse_let(&mut self) -> PResult<Stmt> {
        self.expect(Token::Let, "`let`")?;
        let is_mut = self.eat(&Token::Mut);
        let (name, _) = self.expect_ident("a variable name")?;
        let ty = if self.eat(&Token::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(Token::Assign, "`=`")?;
        self.skip_newlines();
        let value = self.parse_expr()?;
        Ok(Stmt::Let {
            name,
            is_mut,
            ty,
            value,
        })
    }

    fn parse_assign(&mut self) -> PResult<Stmt> {
        let (name, _) = self.expect_ident("a variable name")?;
        self.expect(Token::Assign, "`=`")?;
        self.skip_newlines();
        let value = self.parse_expr()?;
        Ok(Stmt::Assign { name, value })
    }

    fn parse_while(&mut self) -> PResult<Stmt> {
        self.expect(Token::While, "`while`")?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While { cond, body })
    }

    fn parse_return(&mut self) -> PResult<Stmt> {
        self.expect(Token::Return, "`return`")?;
        let value = if matches!(
            self.peek(),
            None | Some(Token::Newline | Token::Semi | Token::RBrace)
        ) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        Ok(Stmt::Return(value))
    }

    // ===== expressions =====

    fn parse_expr(&mut self) -> PResult<Node<Expr>> {
        self.parse_expr_bp(0)
    }

    /// The Pratt loop: keep extending `lhs` while the next operator binds
    /// at least as tightly as `min_bp`.
    fn parse_expr_bp(&mut self, min_bp: u8) -> PResult<Node<Expr>> {
        let mut lhs = self.parse_prefix()?;
        loop {
            // A call `f(...)` binds tighter than anything else.
            if self.at(&Token::LParen) {
                lhs = self.parse_call(lhs)?;
                continue;
            }
            let Some(op) = self.peek().and_then(binary_op) else {
                break;
            };
            let (l_bp, r_bp) = binding_power(op);
            if l_bp < min_bp {
                break;
            }
            self.pos += 1;
            self.skip_newlines(); // a line may end with an operator
            let rhs = self.parse_expr_bp(r_bp)?;
            let span = lhs.span.start..rhs.span.end;
            lhs = Node {
                kind: Expr::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_call(&mut self, callee: Node<Expr>) -> PResult<Node<Expr>> {
        self.expect(Token::LParen, "`(`")?;
        let mut args = Vec::new();
        self.skip_newlines();
        while !self.at(&Token::RParen) {
            args.push(self.parse_expr()?);
            self.skip_newlines();
            if !self.eat(&Token::Comma) {
                break;
            }
            self.skip_newlines();
        }
        self.expect(Token::RParen, "`)`")?;
        let span = callee.span.start..self.prev_end();
        Ok(Node {
            kind: Expr::Call {
                callee: Box::new(callee),
                args,
            },
            span,
        })
    }

    /// Things that can start an expression: literals, names, `-x`, `(x)`, blocks, `if`.
    fn parse_prefix(&mut self) -> PResult<Node<Expr>> {
        let Some(tok) = self.tokens.get(self.pos).cloned() else {
            return self.error("expected an expression, found end of file");
        };
        let span = tok.span.clone();
        match tok.token {
            Token::Int => {
                self.pos += 1;
                let digits = self.text(&span).replace('_', "");
                match digits.parse::<i64>() {
                    Ok(n) => Ok(Node {
                        kind: Expr::Int(n),
                        span,
                    }),
                    Err(_) => Err(ParseError {
                        message: "integer literal is too large".to_string(),
                        span,
                    }),
                }
            }
            Token::Str => {
                self.pos += 1;
                let raw = self.text(&span);
                let inner = raw[1..raw.len() - 1].to_string(); // strip the quotes
                Ok(Node {
                    kind: Expr::Str(inner),
                    span,
                })
            }
            Token::True | Token::False => {
                self.pos += 1;
                Ok(Node {
                    kind: Expr::Bool(tok.token == Token::True),
                    span,
                })
            }
            Token::Ident => {
                self.pos += 1;
                Ok(Node {
                    kind: Expr::Ident(self.text(&span).to_string()),
                    span,
                })
            }
            Token::Minus | Token::Bang => {
                self.pos += 1;
                let op = if tok.token == Token::Minus {
                    UnaryOp::Neg
                } else {
                    UnaryOp::Not
                };
                let operand = self.parse_expr_bp(PREFIX_BP)?;
                let full = span.start..operand.span.end;
                Ok(Node {
                    kind: Expr::Unary {
                        op,
                        operand: Box::new(operand),
                    },
                    span: full,
                })
            }
            Token::LParen => {
                self.pos += 1;
                self.skip_newlines();
                let inner = self.parse_expr()?;
                self.skip_newlines();
                self.expect(Token::RParen, "`)`")?;
                Ok(Node {
                    kind: inner.kind,
                    span: span.start..self.prev_end(),
                })
            }
            Token::LBrace => {
                let block = self.parse_block()?;
                Ok(Node {
                    kind: Expr::Block(block.kind),
                    span: block.span,
                })
            }
            Token::If => self.parse_if(),
            _ => self.error(format!("expected an expression, found {}", self.describe())),
        }
    }

    fn parse_if(&mut self) -> PResult<Node<Expr>> {
        let start = self.expect(Token::If, "`if`")?.start;
        let cond = self.parse_expr()?;
        let then_block = self.parse_block()?;
        let else_branch = if self.eat(&Token::Else) {
            if self.at(&Token::If) {
                Some(Box::new(self.parse_if()?))
            } else {
                let block = self.parse_block()?;
                Some(Box::new(Node {
                    kind: Expr::Block(block.kind),
                    span: block.span,
                }))
            }
        } else {
            None
        };
        Ok(Node {
            kind: Expr::If {
                cond: Box::new(cond),
                then_block,
                else_branch,
            },
            span: start..self.prev_end(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// render an expression as an S expression so tests can check its shape
    fn show(e: &Node<Expr>) -> String {
        match &e.kind {
            Expr::Int(n) => n.to_string(),
            Expr::Bool(b) => b.to_string(),
            Expr::Str(s) => format!("{s:?}"),
            Expr::Ident(name) => name.clone(),
            Expr::Unary { op, operand } => {
                let name = match op {
                    UnaryOp::Neg => "neg",
                    UnaryOp::Not => "not",
                };
                format!("({name} {})", show(operand))
            }
            Expr::Binary { op, lhs, rhs } => format!("({op:?} {} {})", show(lhs), show(rhs)),
            Expr::Call { callee, args } => {
                let mut parts = vec![show(callee)];
                parts.extend(args.iter().map(show));
                format!("(call {})", parts.join(" "))
            }
            Expr::If { .. } => "if".to_string(),
            Expr::Block(_) => "block".to_string(),
        }
    }

    fn parse_main_body(body: &str) -> Block {
        let program = parse(&format!("fn main() {{ {body} }}")).expect("should parse");
        let Item::Fn(f) = &program.items[0].kind;
        f.body.kind.clone()
    }

    /// parse `src` as the tail expression of a function and show it
    fn expr(src: &str) -> String {
        let block = parse_main_body(src);
        show(block.tail.as_ref().expect("expected a tail expression"))
    }

    #[test]
    fn precedence() {
        assert_eq!(expr("1 + 2 * 3"), "(Add 1 (Mul 2 3))");
        assert_eq!(expr("(1 + 2) * 3"), "(Mul (Add 1 2) 3)");
    }

    #[test]
    fn left_associativity() {
        assert_eq!(expr("1 - 2 - 3"), "(Sub (Sub 1 2) 3)");
    }

    #[test]
    fn unary_operators() {
        assert_eq!(expr("-2 + 5"), "(Add (neg 2) 5)");
        assert_eq!(expr("!a && b"), "(And (not a) b)");
        assert_eq!(expr("-f(x)"), "(neg (call f x))");
    }

    #[test]
    fn comparison_and_logic() {
        assert_eq!(expr("1 < 2 && 2 < 3"), "(And (Lt 1 2) (Lt 2 3))");
        assert_eq!(expr("a || b && c"), "(Or a (And b c))");
    }

    #[test]
    fn calls() {
        assert_eq!(expr("f()"), "(call f)");
        assert_eq!(expr("f(1, g(2)) + 3"), "(Add (call f 1 (call g 2)) 3)");
    }

    #[test]
    fn newline_after_operator_continues_the_expression() {
        assert_eq!(expr("1 +\n 2"), "(Add 1 2)");
    }

    #[test]
    fn tail_expression_vs_statement() {
        let with_tail = parse_main_body("let x = 1\n x");
        assert_eq!(with_tail.stmts.len(), 1);
        assert!(with_tail.tail.is_some());

        let with_semi = parse_main_body("x;");
        assert_eq!(with_semi.stmts.len(), 1);
        assert!(with_semi.tail.is_none());
    }

    #[test]
    fn function_header() {
        let program = parse("pub fn load(path: str) -> int uses fs.read, io.print { 1 }").unwrap();
        let Item::Fn(f) = &program.items[0].kind;
        assert!(f.is_pub);
        assert_eq!(f.name, "load");
        assert_eq!(f.params.len(), 1);
        assert!(f.ret.is_some());
        assert_eq!(f.effects.len(), 2);
        assert_eq!(f.effects[0].kind.path, vec!["fs", "read"]);
    }

    #[test]
    fn errors_are_reported() {
        assert!(parse("fn main( {").is_err());
        assert!(parse("fn main() { 1 + }").is_err());
        assert!(parse("fn main() { let x 5 }").is_err());
        assert!(parse("fn main() {").is_err());
    }

    #[test]
    fn all_top_level_examples_parse() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
        let mut count = 0;
        for entry in std::fs::read_dir(dir).expect("examples dir should exist") {
            let path = entry.expect("readable entry").path();
            if path.extension().is_some_and(|e| e == "wd") {
                let src = std::fs::read_to_string(&path).expect("readable file");
                let result = parse(&src);
                assert!(result.is_ok(), "{} failed: {result:?}", path.display());
                count += 1;
            }
        }
        assert!(count > 0, "no examples found");
    }
}
