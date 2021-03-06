//! Parses a series of `lexer` tokens into a code expression.

use std::borrow::Cow::{self, Borrowed, Owned};
use std::collections::HashMap;
use std::fmt;

use num::Num;

use integer::{Integer, Ratio};
use lexer::{Lexer, Span, Token};
use name::{get_standard_name_for, Name, NameDisplay, NameStore};
use string;
use value::Value;

/// Parses a stream of tokens into an expression.
pub struct Parser<'a, 'lex> {
    lexer: Lexer<'lex>,
    names: &'a mut NameStore,
    name_cache: HashMap<&'lex str, Name>,
    cur_token: Option<(Span, Token<'lex>)>,
}

/// Represents an error in parsing input.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    /// Span of source code which caused the error
    pub span: Span,
    /// Kind of error generated
    pub kind: ParseErrorKind,
}

impl ParseError {
    /// Creates a new `ParseError`.
    pub fn new(span: Span, kind: ParseErrorKind) -> ParseError {
        ParseError{
            span: span,
            kind: kind,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.kind, f)
    }
}

impl NameDisplay for ParseError {
    fn fmt(&self, _names: &NameStore, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Describes the kind of error encountered in parsing.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    /// Error in parsing literal
    InvalidLiteral,
    /// Error in parsing token
    InvalidToken,
    /// Invalid character in input
    InvalidChar(char),
    /// Invalid character in numeric escape sequence `\xNN` or `\u{NNNN}`
    InvalidNumericEscape(char),
    /// Error parsing literal string into value
    LiteralParseError,
    /// Missing closing parenthesis
    MissingCloseParen,
    /// More commas than backquotes
    UnbalancedComma,
    /// Unexpected end-of-file
    UnexpectedEof,
    /// Unexpected token
    UnexpectedToken{
        /// Token or category of token expected
        expected: &'static str,
        /// Token found
        found: &'static str,
    },
    /// Unrecognized character escape
    UnknownCharEscape(char),
    /// Unmatched `)`
    UnmatchedParen,
    /// Unterminated character constant
    UnterminatedChar,
    /// Unterminated block comment
    UnterminatedComment,
    /// Unterminated string constant
    UnterminatedString,
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ParseErrorKind::InvalidLiteral => f.write_str("invalid numeric literal"),
            ParseErrorKind::InvalidToken => f.write_str("invalid token"),
            ParseErrorKind::InvalidChar(ch) =>
                write!(f, "invalid character: {:?}", ch),
            ParseErrorKind::InvalidNumericEscape(ch) =>
                write!(f, "invalid character in {} escape sequence", ch),
            ParseErrorKind::LiteralParseError => f.write_str("literal parse error"),
            ParseErrorKind::MissingCloseParen => f.write_str("missing close paren"),
            ParseErrorKind::UnbalancedComma => f.write_str("unbalanced ` and ,"),
            ParseErrorKind::UnexpectedEof => f.write_str("unexpected end-of-file"),
            ParseErrorKind::UnexpectedToken{expected, found} =>
                write!(f, "expected {}; found {}", expected, found),
            ParseErrorKind::UnknownCharEscape(ch) =>
                write!(f, "unknown char escape: {:?}", ch),
            ParseErrorKind::UnmatchedParen => f.write_str("unmatched `)`"),
            ParseErrorKind::UnterminatedChar => f.write_str("unterminated char constant"),
            ParseErrorKind::UnterminatedComment => f.write_str("unterminated block comment"),
            ParseErrorKind::UnterminatedString => f.write_str("unterminated string constant"),
        }
    }
}

enum Group {
    /// Positive indicates a number of backticks,
    /// negative indicates a number of commas.
    Backticks(i32),
    CommaAt,
    /// Number of quotes preceding group.
    /// If zero, this is an unquoted parentheses group.
    Quotes(u32),
    /// Values in a parenthetical expression
    Parens(Vec<Value>),
}

impl<'a, 'lex> Parser<'a, 'lex> {
    /// Creates a new `Parser` using the given `Lexer`.
    /// Identifiers received from the lexer will be inserted into the given
    /// `NameStore`.
    pub fn new(names: &'a mut NameStore, lexer: Lexer<'lex>) -> Parser<'a, 'lex> {
        Parser{
            lexer: lexer,
            names: names,
            name_cache: HashMap::new(),
            cur_token: None,
        }
    }

    /// Skips the "shebang" line of a source file.
    pub fn skip_shebang(&mut self) {
        self.lexer.skip_shebang();
    }

    /// Parses an expression from the input stream.
    pub fn parse_expr(&mut self) -> Result<Value, ParseError> {
        let mut stack = Vec::new();
        let mut total_backticks = 0;

        loop {
            let (sp, tok) = try!(self.next());

            let r = match tok {
                Token::DocComment(_) => unreachable!(),
                Token::LeftParen => {
                    stack.push(Group::Parens(Vec::new()));
                    continue;
                }
                Token::RightParen => {
                    let group = try!(stack.pop().ok_or_else(
                        || ParseError::new(sp, ParseErrorKind::UnmatchedParen)));

                    match group {
                        Group::Parens(values) => Ok(values.into()),
                        _ => Err(ParseError::new(sp,
                            ParseErrorKind::UnexpectedToken{
                                expected: "expression",
                                found: ")",
                            }))
                    }
                }
                Token::Float(f) => parse_float(f)
                    .map(|f| Value::Float(f))
                    .map_err(|kind| ParseError::new(sp, kind)),
                Token::Integer(i, base) => parse_integer(i, base)
                    .map(|i| Value::Integer(i))
                    .map_err(|kind| ParseError::new(sp, kind)),
                Token::Ratio(r) => parse_ratio(r)
                    .map(|r| Value::Ratio(r))
                    .map_err(|_| ParseError::new(sp, ParseErrorKind::LiteralParseError)),
                Token::Char(ch) => parse_char(ch)
                    .map(|ch| Value::Char(ch)),
                Token::String(s) => parse_string(s)
                    .map(|s| Value::String(s)),
                Token::Name(name) => Ok(self.name_value(name)),
                Token::Keyword(name) => Ok(Value::Keyword(self.add_name(name))),
                Token::BackQuote => {
                    total_backticks += 1;
                    if let Some(&mut Group::Backticks(ref mut n)) = stack.last_mut() {
                        *n += 1;
                        continue;
                    }
                    stack.push(Group::Backticks(1));
                    continue;
                }
                Token::Comma => {
                    if total_backticks <= 0 {
                        return Err(ParseError::new(sp, ParseErrorKind::UnbalancedComma));
                    }
                    total_backticks -= 1;
                    if let Some(&mut Group::Backticks(ref mut n)) = stack.last_mut() {
                        *n -= 1;
                        continue;
                    }
                    stack.push(Group::Backticks(-1));
                    continue;
                }
                Token::CommaAt => {
                    if total_backticks <= 0 {
                        return Err(ParseError::new(sp, ParseErrorKind::UnbalancedComma));
                    }
                    total_backticks -= 1;
                    stack.push(Group::CommaAt);
                    continue;
                }
                Token::Quote => {
                    if let Some(&mut Group::Quotes(ref mut n)) = stack.last_mut() {
                        *n += 1;
                        continue;
                    }
                    stack.push(Group::Quotes(1));
                    continue;
                }
                Token::End => {
                    let any_paren = stack.iter().any(|group| {
                        match *group {
                            Group::Parens(_) => true,
                            _ => false
                        }
                    });

                    if any_paren {
                        Err(ParseError::new(sp,
                            ParseErrorKind::MissingCloseParen))
                    } else {
                        Err(ParseError::new(sp,
                            ParseErrorKind::UnexpectedEof))
                    }
                }
            };

            let mut v = try!(r);

            loop {
                match stack.last_mut() {
                    None => return Ok(v),
                    Some(&mut Group::Parens(ref mut values)) => {
                        values.push(v);
                        break;
                    }
                    _ => ()
                }

                let group = stack.pop().unwrap();

                match group {
                    // 0 backticks is ignored, but must still be considered
                    // a group because an expression must follow.
                    Group::Backticks(n) if n > 0 => {
                        total_backticks -= n;
                        v = v.quasiquote(n as u32);
                    }
                    Group::Backticks(n) if n < 0 => {
                        total_backticks -= n; // Subtracting a negative
                        v = v.comma((-n) as u32);
                    }
                    Group::CommaAt => {
                        total_backticks += 1;
                        v = v.comma_at(1);
                    }
                    Group::Quotes(n) => v = v.quote(n),
                    _ => ()
                }
            }
        }
    }

    /// Parses a single expression from the input stream.
    /// If any tokens remain after the expression, an error is returned.
    pub fn parse_single_expr(&mut self) -> Result<Value, ParseError> {
        let expr = try!(self.parse_expr());

        match try!(self.next()) {
            (_, Token::End) => Ok(expr),
            (sp, tok) => Err(ParseError::new(sp, ParseErrorKind::UnexpectedToken{
                expected: "eof",
                found: tok.name(),
            }))
        }
    }

    /// Parse a series of expressions from the input stream.
    pub fn parse_exprs(&mut self) -> Result<Vec<Value>, ParseError> {
        let mut res = Vec::new();

        loop {
            match try!(self.peek()) {
                (_sp, Token::End) => break,
                _ => res.push(try!(self.parse_expr()))
            }
        }

        Ok(res)
    }

    /// Returns the the next token if it is a doc comment.
    /// Otherwise, `None` is returned and the token will be processed later.
    pub fn read_doc_comment(&mut self) -> Result<Option<&'lex str>, ParseError> {
        match try!(self.peek_all()) {
            (_, Token::DocComment(doc)) => Ok(Some(doc)),
            _ => Ok(None)
        }
    }

    fn add_name(&mut self, name: &'lex str) -> Name {
        let names = &mut *self.names;
        *self.name_cache.entry(name).or_insert_with(
            || get_standard_name_for(name).unwrap_or_else(|| names.add(name)))
    }

    fn name_value(&mut self, name: &'lex str) -> Value {
        match name {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => {
                let name = self.add_name(name);
                Value::Name(name)
            }
        }
    }

    fn next(&mut self) -> Result<(Span, Token<'lex>), ParseError> {
        let r = try!(self.peek_all());
        self.cur_token = None;
        Ok(r)
    }

    /// Returns the next non-`DocComment` token without consuming it
    fn peek(&mut self) -> Result<(Span, Token<'lex>), ParseError> {
        loop {
            match try!(self.peek_all()) {
                (_, Token::DocComment(_)) => { self.cur_token.take(); }
                tok => return Ok(tok)
            }
        }
    }

    /// Returns the next token without consuming it
    fn peek_all(&mut self) -> Result<(Span, Token<'lex>), ParseError> {
        if let Some(tok) = self.cur_token.clone() {
            Ok(tok)
        } else {
            let tok = try!(self.lexer.next_token());
            self.cur_token = Some(tok);
            Ok(tok)
        }
    }
}

fn parse_char(s: &str) -> Result<char, ParseError> {
    let (ch, _) = try!(string::parse_char(s, 0));
    Ok(ch)
}

fn parse_string(s: &str) -> Result<String, ParseError> {
    let (s, _) = if s.starts_with('r') {
        try!(string::parse_raw_string(s, 0))
    } else {
        try!(string::parse_string(s, 0))
    };
    Ok(s)
}

fn parse_float(s: &str) -> Result<f64, ParseErrorKind> {
    strip_underscores(s).parse()
        .map_err(|_| ParseErrorKind::LiteralParseError)
}

fn parse_integer(s: &str, base: u32) -> Result<Integer, ParseErrorKind> {
    let s = match base {
        10 => s,
        _ => &s[2..]
    };

    Integer::from_str_radix(&strip_underscores(s), base)
        .map_err(|_| ParseErrorKind::LiteralParseError)
}

fn parse_ratio(s: &str) -> Result<Ratio, ParseErrorKind> {
    strip_underscores(s).parse()
        .map_err(|_| ParseErrorKind::LiteralParseError)
}

fn strip_underscores(s: &str) -> Cow<str> {
    if s.contains('_') {
        Owned(s.chars().filter(|&ch| ch != '_').collect())
    } else {
        Borrowed(s)
    }
}

#[cfg(test)]
mod test {
    use super::{ParseError, ParseErrorKind, Parser};
    use lexer::{Span, Lexer};
    use name::NameStore;
    use value::Value;

    fn parse(s: &str) -> Result<Value, ParseError> {
        let mut names = NameStore::new();
        let mut p = Parser::new(&mut names, Lexer::new(s, 0));
        p.parse_expr()
    }

    #[test]
    fn test_errors() {
        assert_eq!(parse("(foo").unwrap_err(), ParseError{
            span: Span{lo: 4, hi: 4}, kind: ParseErrorKind::MissingCloseParen});
        assert_eq!(parse("(foo ,bar)").unwrap_err(), ParseError{
            span: Span{lo: 5, hi: 6}, kind: ParseErrorKind::UnbalancedComma});
        assert_eq!(parse("`(foo ,,bar)").unwrap_err(), ParseError{
            span: Span{lo: 7, hi: 8}, kind: ParseErrorKind::UnbalancedComma});
    }
}
