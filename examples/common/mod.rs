//! A miniature calculator language shared by the examples.
//!
//! It is small but complete enough to show how a real language plugs into a
//! `repl_lang::Session`:
//!
//! - the lexer runs on `Input::cursor`, so every token span is a global span in
//!   the session's source map;
//! - the parser is `parser_lang`'s Pratt engine, and `Input::is_incomplete`
//!   turns its end-of-input errors into continuation lines;
//! - bindings remember where they were defined, so a diagnostic in one entry
//!   can point at a definition made several entries earlier.
//!
//! Grammar:
//!
//! ```text
//! entry := "let" NAME "=" expr | expr
//! expr  := NUMBER | NAME | "(" expr ")" | "-" expr | expr OP expr
//! OP    := "+" | "-" | "*" | "/" | "%" | "^"      (^ is right-associative)
//! ```
//!
//! Numbers are `i64` and every operation is checked, so overflow and division
//! by zero are diagnostics rather than panics. Whitespace, newlines included,
//! is insignificant, and `#` starts a comment that runs to the end of the line.

use std::collections::HashMap;

use diag_lang::{Diagnostic, Label, Severity};
use parser_lang::{Parser, Pratt, Span, Token, TokenKind};
use repl_lang::{Input, Status};

/// What a successfully evaluated entry produced.
#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    /// An expression's value.
    Value(i64),
    /// A new binding, `let name = value`.
    Bound { name: String, value: i64 },
}

/// The interpreter state that persists across entries.
#[derive(Default)]
pub struct Calc {
    /// Each binding's value and the global span of its name in the `let`.
    bindings: HashMap<String, (i64, Span)>,
    /// Token buffer reused across entries and continuation lines.
    tokens: Vec<Token<Kind>>,
}

impl Calc {
    /// Creates an interpreter with no bindings.
    pub fn new() -> Self {
        Self::default()
    }

    /// The pipeline: lex, parse, and — once the entry is complete — evaluate.
    ///
    /// Pass it straight to `Session::feed`:
    /// `session.feed(line, |input| calc.run(input))`.
    pub fn run(&mut self, input: Input<'_>) -> Status<Result<Reply, Vec<Diagnostic>>> {
        lex(input, &mut self.tokens);
        let mut parser = Parser::new(&self.tokens);
        let entry = parse_entry(&mut parser, input);
        let errors = parser.into_errors();

        if input.is_incomplete(&errors) {
            return Status::Incomplete;
        }
        let result = match entry {
            Some(entry) if errors.is_empty() => self.eval_entry(entry, input),
            _ => Err(errors),
        };
        Status::Complete(result)
    }

    fn eval_entry(&mut self, entry: Entry, input: Input<'_>) -> Result<Reply, Vec<Diagnostic>> {
        match entry {
            Entry::Expr(expr) => self
                .eval(&expr, input)
                .map(Reply::Value)
                .map_err(|d| vec![d]),
            Entry::Let { name, value } => {
                let text = slice(input, name).to_owned();
                if let Some(&(_, first)) = self.bindings.get(&text) {
                    return Err(vec![
                        Diagnostic::new(
                            Severity::Error,
                            format!("`{text}` is already defined"),
                            Label::new(name, "defined again here"),
                        )
                        .with_secondary(Label::new(first, "first defined here"))
                        .with_help("bindings are immutable; choose another name"),
                    ]);
                }
                let value = self.eval(&value, input).map_err(|d| vec![d])?;
                let _previous = self.bindings.insert(text.clone(), (value, name));
                Ok(Reply::Bound { name: text, value })
            }
        }
    }

    fn eval(&self, expr: &Expr, input: Input<'_>) -> Result<i64, Diagnostic> {
        match expr {
            Expr::Num { value, .. } => Ok(*value),
            Expr::Var { span } => {
                let name = slice(input, *span);
                self.bindings
                    .get(name)
                    .map(|&(value, _)| value)
                    .ok_or_else(|| {
                        Diagnostic::new(
                            Severity::Error,
                            format!("cannot find `{name}`"),
                            Label::new(*span, "not defined"),
                        )
                        .with_help(format!("define it first: `let {name} = ...`"))
                    })
            }
            Expr::Neg { operand, span } => {
                let value = self.eval(operand, input)?;
                value.checked_neg().ok_or_else(|| overflow(*span))
            }
            Expr::Binary {
                op,
                op_span,
                left,
                right,
            } => {
                let l = self.eval(left, input)?;
                let r = self.eval(right, input)?;
                let span = left.span().merge(right.span());
                let zero = || {
                    Diagnostic::new(
                        Severity::Error,
                        "division by zero",
                        Label::new(*op_span, "cannot divide"),
                    )
                    .with_secondary(Label::new(right.span(), "this is zero"))
                };
                match op {
                    Kind::Plus => l.checked_add(r).ok_or_else(|| overflow(span)),
                    Kind::Minus => l.checked_sub(r).ok_or_else(|| overflow(span)),
                    Kind::Star => l.checked_mul(r).ok_or_else(|| overflow(span)),
                    Kind::Slash if r == 0 => Err(zero()),
                    Kind::Slash => l.checked_div(r).ok_or_else(|| overflow(span)),
                    Kind::Percent if r == 0 => Err(zero()),
                    Kind::Percent => l.checked_rem(r).ok_or_else(|| overflow(span)),
                    _ => match u32::try_from(r) {
                        Ok(exp) => l.checked_pow(exp).ok_or_else(|| overflow(span)),
                        Err(_) => Err(Diagnostic::new(
                            Severity::Error,
                            "exponent out of range",
                            Label::new(right.span(), format!("{r} is not a valid exponent")),
                        )),
                    },
                }
            }
        }
    }
}

fn overflow(span: Span) -> Diagnostic {
    Diagnostic::new(
        Severity::Error,
        "arithmetic overflow",
        Label::new(span, "result does not fit in 64 bits"),
    )
}

/// The text a global span covers, recovered from the entry being evaluated.
fn slice(input: Input<'_>, span: Span) -> &str {
    let base = input.base().to_usize();
    &input.text()[span.start().to_usize() - base..span.end().to_usize() - base]
}

/// Token kinds of the calculator language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Num,
    Name,
    Let,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
    Space,
    Comment,
    Unknown,
    Eof,
}

impl TokenKind for Kind {
    fn is_trivia(&self) -> bool {
        matches!(self, Kind::Space | Kind::Comment)
    }

    fn is_eof(&self) -> bool {
        matches!(self, Kind::Eof)
    }
}

/// Lexes the whole entry into `tokens`. The spans come out global because the
/// cursor starts at the entry's base.
fn lex(input: Input<'_>, tokens: &mut Vec<Token<Kind>>) {
    tokens.clear();
    let mut cursor = input.cursor();
    while let Some(c) = cursor.bump() {
        let kind = match c {
            c if c.is_whitespace() => {
                cursor.eat_while(char::is_whitespace);
                Kind::Space
            }
            '#' => {
                cursor.eat_while(|c| c != '\n');
                Kind::Comment
            }
            '0'..='9' => {
                cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
                Kind::Num
            }
            c if c.is_alphabetic() || c == '_' => {
                cursor.eat_while(|c| c.is_alphanumeric() || c == '_');
                if cursor.lexeme() == "let" {
                    Kind::Let
                } else {
                    Kind::Name
                }
            }
            '=' => Kind::Assign,
            '+' => Kind::Plus,
            '-' => Kind::Minus,
            '*' => Kind::Star,
            '/' => Kind::Slash,
            '%' => Kind::Percent,
            '^' => Kind::Caret,
            '(' => Kind::LParen,
            ')' => Kind::RParen,
            _ => Kind::Unknown,
        };
        tokens.push(cursor.emit(kind));
    }
    tokens.push(Token::new(Kind::Eof, Span::empty(cursor.pos().to_u32())));
}

/// A parsed entry.
enum Entry {
    Expr(Expr),
    Let { name: Span, value: Expr },
}

/// A parsed expression. Every node keeps the global span it came from.
enum Expr {
    Num {
        value: i64,
        span: Span,
    },
    Var {
        span: Span,
    },
    Neg {
        operand: Box<Expr>,
        span: Span,
    },
    Binary {
        op: Kind,
        op_span: Span,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

impl Expr {
    fn span(&self) -> Span {
        match self {
            Expr::Num { span, .. } | Expr::Var { span } | Expr::Neg { span, .. } => *span,
            Expr::Binary { left, right, .. } => left.span().merge(right.span()),
        }
    }
}

/// `entry := "let" NAME "=" expr | expr`, then the end of input.
fn parse_entry(p: &mut Parser<'_, Kind>, input: Input<'_>) -> Option<Entry> {
    let mut grammar = Grammar { input };
    let entry = if p.eat(|k| *k == Kind::Let).is_some() {
        let name = p.expect(|k| *k == Kind::Name, "a name")?.span();
        let _assign = p.expect(|k| *k == Kind::Assign, "`=`")?;
        let value = grammar.parse(p)?;
        Entry::Let { name, value }
    } else {
        Entry::Expr(grammar.parse(p)?)
    };
    if !p.at_end() {
        p.error("expected an operator or the end of the entry");
        return None;
    }
    Some(entry)
}

/// The Pratt grammar for expressions.
struct Grammar<'a> {
    input: Input<'a>,
}

impl<'t> Pratt<'t, Kind> for Grammar<'_> {
    type Output = Expr;

    fn prefix(&mut self, p: &mut Parser<'t, Kind>) -> Option<Expr> {
        if p.at_end() {
            p.error("expected an expression");
            return None;
        }
        let token = p.bump()?;
        let span = token.span();
        match token.kind() {
            Kind::Num => match slice(self.input, span).replace('_', "").parse::<i64>() {
                Ok(value) => Some(Expr::Num { value, span }),
                Err(_) => {
                    p.error_at(span, "number does not fit in 64 bits");
                    None
                }
            },
            Kind::Name => Some(Expr::Var { span }),
            Kind::Minus => {
                // Binds tighter than `*` but looser than `^`: -2^2 is -(2^2).
                let operand = self.expression(p, 5)?;
                let span = span.merge(operand.span());
                Some(Expr::Neg {
                    operand: Box::new(operand),
                    span,
                })
            }
            Kind::LParen => {
                let inner = self.expression(p, 0)?;
                let _close = p.expect(|k| *k == Kind::RParen, "`)`")?;
                Some(inner)
            }
            Kind::Unknown => {
                p.error_at(span, "unexpected character");
                None
            }
            _ => {
                p.error_at(span, "expected an expression");
                None
            }
        }
    }

    fn infix_binding(&self, kind: &Kind) -> Option<(u8, u8)> {
        match kind {
            Kind::Plus | Kind::Minus => Some((1, 2)),
            Kind::Star | Kind::Slash | Kind::Percent => Some((3, 4)),
            Kind::Caret => Some((7, 6)),
            _ => None,
        }
    }

    fn infix(&mut self, op: &'t Token<Kind>, left: Expr, right: Expr) -> Option<Expr> {
        Some(Expr::Binary {
            op: *op.kind(),
            op_span: op.span(),
            left: Box::new(left),
            right: Box::new(right),
        })
    }
}
