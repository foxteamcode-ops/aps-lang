use crate::compiler::diagnostics::{Diagnostic, DiagnosticBag, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    IntLit(i64, Option<String>),
    FloatLit(f64, Option<String>),
    StringLit(String),
    FormatString(String),
    CharLit(char),
    BoolLit(bool),
    Ident(String),

    // Keywords
    Fn,
    Let,
    Var,
    Const,
    Struct,
    Enum,
    Trait,
    Impl,
    Type,
    Module,
    Import,
    Pub,
    Extern,
    If,
    Else,
    Match,
    While,
    For,
    In,
    Loop,
    Break,
    Continue,
    Return,
    Defer,
    Read,
    Write,
    Take,
    Region,
    Share,
    Shared,
    Unsafe,
    Async,
    Await,
    Spawn,
    As,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    AndAnd,
    OrOr,
    Bang,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    Dot,
    DotDot,
    DotDotEq,
    ColonColon,
    Arrow,    // ->
    FatArrow, // =>
    Question, // ?
    At,       // @

    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semi,

    // Doc comment
    DocComment(String),

    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub preceded_by_newline: bool,
}

pub struct Lexer<'a> {
    file: String,
    src: &'a str,
    chars: Vec<(usize, char)>,
    pos: usize,
    line: usize,
    col: usize,
    pub diagnostics: DiagnosticBag,
}

impl<'a> Lexer<'a> {
    pub fn new(file: &str, src: &'a str) -> Self {
        // Tolerate a UTF-8 byte-order mark (common with Windows editors/PowerShell).
        let src = src.strip_prefix('\u{feff}').unwrap_or(src);
        Self {
            file: file.to_string(),
            src,
            chars: src.char_indices().collect(),
            pos: 0,
            line: 1,
            col: 1,
            diagnostics: DiagnosticBag::new(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).map(|(_, c)| *c)
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).map(|(_, c)| *c)
    }

    fn current_byte_pos(&self) -> usize {
        self.chars
            .get(self.pos)
            .map(|(b, _)| *b)
            .unwrap_or(self.src.len())
    }

    fn advance(&mut self) -> Option<char> {
        if let Some((_, ch)) = self.chars.get(self.pos).copied() {
            self.pos += 1;
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            Some(ch)
        } else {
            None
        }
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut saw_newline = true;

        while self.pos < self.chars.len() {
            let ch = self.peek().unwrap();

            // Whitespace
            if ch.is_whitespace() {
                if ch == '\n' {
                    saw_newline = true;
                }
                self.advance();
                continue;
            }

            // Comments
            if ch == '/' && self.peek_next() == Some('/') {
                let start_line = self.line;
                let start_col = self.col;
                let start_byte = self.current_byte_pos();
                self.advance();
                self.advance();
                if self.peek() == Some('/') || self.peek() == Some('!') {
                    self.advance();
                    let mut doc = String::new();
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        doc.push(c);
                        self.advance();
                    }
                    let end_byte = self.current_byte_pos();
                    let _ = (start_line, start_col, start_byte, end_byte, doc);
                } else {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                continue;
            }

            if ch == '/' && self.peek_next() == Some('*') {
                let start_line = self.line;
                let start_col = self.col;
                let start_byte = self.current_byte_pos();
                self.advance();
                self.advance();
                let mut depth = 1usize;
                while depth > 0 {
                    match (self.peek(), self.peek_next()) {
                        (Some('/'), Some('*')) => {
                            self.advance();
                            self.advance();
                            depth += 1;
                        }
                        (Some('*'), Some('/')) => {
                            self.advance();
                            self.advance();
                            depth -= 1;
                        }
                        (Some(_), _) => {
                            self.advance();
                        }
                        (None, _) => {
                            let span = Span::new(
                                &self.file,
                                start_line,
                                start_col,
                                start_byte,
                                self.current_byte_pos(),
                            );
                            self.diagnostics.push(
                                Diagnostic::error("E0002", "unterminated block comment", span)
                                    .with_help("close the comment with `*/`"),
                            );
                            break;
                        }
                    }
                }
                continue;
            }

            let start_line = self.line;
            let start_col = self.col;
            let start_byte = self.current_byte_pos();
            let preceded_by_newline = saw_newline;
            saw_newline = false;

            // Format string literal f"..."
            if (ch == 'f' || ch == 'F') && self.peek_next() == Some('"') {
                self.advance(); // consume 'f'
                let kind = self.lex_fstring(start_line, start_col, start_byte);
                let end_byte = self.current_byte_pos();
                tokens.push(Token {
                    kind,
                    span: Span::new(&self.file, start_line, start_col, start_byte, end_byte),
                    preceded_by_newline,
                });
                continue;
            }

            // String literal
            if ch == '"' {
                let kind = self.lex_string(start_line, start_col, start_byte);
                let end_byte = self.current_byte_pos();
                tokens.push(Token {
                    kind,
                    span: Span::new(&self.file, start_line, start_col, start_byte, end_byte),
                    preceded_by_newline,
                });
                continue;
            }

            // Character literal
            if ch == '\'' {
                let kind = self.lex_char(start_line, start_col, start_byte);
                let end_byte = self.current_byte_pos();
                tokens.push(Token {
                    kind,
                    span: Span::new(&self.file, start_line, start_col, start_byte, end_byte),
                    preceded_by_newline,
                });
                continue;
            }

            // Number literal
            if ch.is_ascii_digit() {
                let kind = self.lex_number(start_line, start_col, start_byte);
                let end_byte = self.current_byte_pos();
                tokens.push(Token {
                    kind,
                    span: Span::new(&self.file, start_line, start_col, start_byte, end_byte),
                    preceded_by_newline,
                });
                continue;
            }

            // Identifier or Keyword
            if ch.is_alphabetic() || ch == '_' {
                let kind = self.lex_ident_or_keyword();
                let end_byte = self.current_byte_pos();
                tokens.push(Token {
                    kind,
                    span: Span::new(&self.file, start_line, start_col, start_byte, end_byte),
                    preceded_by_newline,
                });
                continue;
            }

            // Operators & Punctuation
            let kind = self.lex_operator_or_punct(start_line, start_col, start_byte);
            let end_byte = self.current_byte_pos();
            tokens.push(Token {
                kind,
                span: Span::new(&self.file, start_line, start_col, start_byte, end_byte),
                preceded_by_newline,
            });
        }

        let eof_pos = self.src.len();
        tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::new(&self.file, self.line, self.col, eof_pos, eof_pos),
            preceded_by_newline: true,
        });

        tokens
    }

    fn lex_string(&mut self, start_line: usize, start_col: usize, start_byte: usize) -> TokenKind {
        self.advance(); // opening '"'
        let mut s = String::new();
        let mut closed = false;

        while let Some(c) = self.peek() {
            if c == '"' {
                self.advance();
                closed = true;
                break;
            }
            if c == '\\' {
                self.advance();
                match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('r') => s.push('\r'),
                    Some('t') => s.push('\t'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some('0') => s.push('\0'),
                    Some('\'') => s.push('\''),
                    Some(other) => {
                        s.push(other);
                    }
                    None => break,
                }
            } else {
                s.push(c);
                self.advance();
            }
        }

        if !closed {
            let span = Span::new(
                &self.file,
                start_line,
                start_col,
                start_byte,
                self.current_byte_pos(),
            );
            self.diagnostics.push(
                Diagnostic::error("E0003", "unterminated string literal", span)
                    .with_help("add a closing double quote `\"`"),
            );
        }

        TokenKind::StringLit(s)
    }

    fn lex_fstring(&mut self, start_line: usize, start_col: usize, start_byte: usize) -> TokenKind {
        self.advance(); // opening '"'
        let mut s = String::new();
        let mut closed = false;

        while let Some(c) = self.advance() {
            match c {
                '"' => {
                    closed = true;
                    break;
                }
                '\\' => match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('r') => s.push('\r'),
                    Some('t') => s.push('\t'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some('0') => s.push('\0'),
                    Some('{') => s.push('{'),
                    Some('}') => s.push('}'),
                    Some(escaped) => s.push(escaped),
                    None => break,
                },
                other => s.push(other),
            }
        }

        if !closed {
            let span = Span::new(
                &self.file,
                start_line,
                start_col,
                start_byte,
                self.current_byte_pos(),
            );
            self.diagnostics.push(
                Diagnostic::error("E0003", "unterminated format string literal", span)
                    .with_help("add a closing double quote `\"`"),
            );
        }

        TokenKind::FormatString(s)
    }

    fn lex_char(&mut self, start_line: usize, start_col: usize, start_byte: usize) -> TokenKind {
        self.advance(); // opening '\''
        let ch = match self.advance() {
            Some('\\') => match self.advance() {
                Some('n') => '\n',
                Some('r') => '\r',
                Some('t') => '\t',
                Some('\\') => '\\',
                Some('\'') => '\'',
                Some('0') => '\0',
                Some(c) => c,
                None => '\0',
            },
            Some(c) => c,
            None => '\0',
        };
        if self.peek() == Some('\'') {
            self.advance();
        } else {
            let span = Span::new(
                &self.file,
                start_line,
                start_col,
                start_byte,
                self.current_byte_pos(),
            );
            self.diagnostics.push(
                Diagnostic::error("E0004", "unterminated or invalid character literal", span)
                    .with_help("character literals must contain a single character enclosed in `'x'`"),
            );
        }
        TokenKind::CharLit(ch)
    }

    fn lex_number(&mut self, start_line: usize, start_col: usize, start_byte: usize) -> TokenKind {
        let mut raw = String::new();
        let mut radix = 10u32;

        if self.peek() == Some('0') {
            match self.peek_next() {
                Some('x') | Some('X') => {
                    self.advance();
                    self.advance();
                    radix = 16;
                }
                Some('b') | Some('B') => {
                    self.advance();
                    self.advance();
                    radix = 2;
                }
                Some('o') | Some('O') => {
                    self.advance();
                    self.advance();
                    radix = 8;
                }
                _ => {}
            }
        }

        let mut is_float = false;

        while let Some(c) = self.peek() {
            if c == '_' {
                self.advance();
                continue;
            }
            if c.is_digit(radix) {
                raw.push(c);
                self.advance();
            } else if radix == 10 && c == '.' && !is_float {
                // Make sure it's not `..` range or method call `.len()`
                if let Some(next_c) = self.peek_next() {
                    if next_c.is_ascii_digit() {
                        is_float = true;
                        raw.push('.');
                        self.advance();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else if radix == 10 && (c == 'e' || c == 'E') {
                is_float = true;
                raw.push(c);
                self.advance();
                if let Some(sign) = self.peek() {
                    if sign == '+' || sign == '-' {
                        raw.push(sign);
                        self.advance();
                    }
                }
            } else {
                break;
            }
        }

        // Optional type suffix like i32, i64, u64, f64, usize
        let mut suffix = String::new();
        if let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                while let Some(sc) = self.peek() {
                    if sc.is_ascii_alphanumeric() || sc == '_' {
                        suffix.push(sc);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }

        let suffix_opt = if suffix.is_empty() {
            None
        } else {
            Some(suffix.clone())
        };

        if is_float || suffix == "f32" || suffix == "f64" {
            match raw.parse::<f64>() {
                Ok(val) => TokenKind::FloatLit(val, suffix_opt),
                Err(_) => {
                    let span = Span::new(
                        &self.file,
                        start_line,
                        start_col,
                        start_byte,
                        self.current_byte_pos(),
                    );
                    self.diagnostics
                        .push(Diagnostic::error("E0005", "invalid float literal", span));
                    TokenKind::FloatLit(0.0, suffix_opt)
                }
            }
        } else {
            let parsed = i128::from_str_radix(&raw, radix);
            match parsed {
                Ok(val) => TokenKind::IntLit(val as i64, suffix_opt),
                Err(_) => {
                    let span = Span::new(
                        &self.file,
                        start_line,
                        start_col,
                        start_byte,
                        self.current_byte_pos(),
                    );
                    self.diagnostics
                        .push(Diagnostic::error("E0005", "invalid integer literal", span));
                    TokenKind::IntLit(0, suffix_opt)
                }
            }
        }
    }

    fn lex_ident_or_keyword(&mut self) -> TokenKind {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }

        match s.as_str() {
            "fn" => TokenKind::Fn,
            "let" => TokenKind::Let,
            "var" => TokenKind::Var,
            "const" => TokenKind::Const,
            "struct" => TokenKind::Struct,
            "enum" => TokenKind::Enum,
            "trait" => TokenKind::Trait,
            "impl" => TokenKind::Impl,
            "type" => TokenKind::Type,
            "module" => TokenKind::Module,
            "import" => TokenKind::Import,
            "pub" => TokenKind::Pub,
            "extern" => TokenKind::Extern,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "match" => TokenKind::Match,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "loop" => TokenKind::Loop,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "return" => TokenKind::Return,
            "defer" => TokenKind::Defer,
            "read" => TokenKind::Read,
            "write" => TokenKind::Write,
            "take" => TokenKind::Take,
            "region" => TokenKind::Region,
            "share" => TokenKind::Share,
            "shared" => TokenKind::Shared,
            "unsafe" => TokenKind::Unsafe,
            "async" => TokenKind::Async,
            "await" => TokenKind::Await,
            "spawn" => TokenKind::Spawn,
            "true" => TokenKind::BoolLit(true),
            "false" => TokenKind::BoolLit(false),
            "as" => TokenKind::As,
            _ => TokenKind::Ident(s),
        }
    }

    fn lex_operator_or_punct(
        &mut self,
        start_line: usize,
        start_col: usize,
        start_byte: usize,
    ) -> TokenKind {
        let ch = self.advance().unwrap();
        match ch {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            ';' => TokenKind::Semi,
            '?' => TokenKind::Question,
            '~' => TokenKind::Tilde,
            '@' => TokenKind::At,
            '^' => TokenKind::Caret,
            ':' => {
                if self.peek() == Some(':') {
                    self.advance();
                    TokenKind::ColonColon
                } else {
                    TokenKind::Colon
                }
            }
            '.' => {
                if self.peek() == Some('.') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::DotDotEq
                    } else {
                        TokenKind::DotDot
                    }
                } else {
                    TokenKind::Dot
                }
            }
            '+' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::PlusEq
                } else {
                    TokenKind::Plus
                }
            }
            '-' => {
                if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Arrow
                } else if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::MinusEq
                } else {
                    TokenKind::Minus
                }
            }
            '*' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::StarEq
                } else {
                    TokenKind::Star
                }
            }
            '/' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::SlashEq
                } else {
                    TokenKind::Slash
                }
            }
            '%' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::PercentEq
                } else {
                    TokenKind::Percent
                }
            }
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::EqEq
                } else if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::FatArrow
                } else {
                    TokenKind::Eq
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::NotEq
                } else {
                    TokenKind::Bang
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::LtEq
                } else if self.peek() == Some('<') {
                    self.advance();
                    TokenKind::Shl
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::GtEq
                } else if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Shr
                } else {
                    TokenKind::Gt
                }
            }
            '&' => {
                if self.peek() == Some('&') {
                    self.advance();
                    TokenKind::AndAnd
                } else {
                    TokenKind::Amp
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.advance();
                    TokenKind::OrOr
                } else {
                    TokenKind::Pipe
                }
            }
            _ => {
                let span = Span::new(
                    &self.file,
                    start_line,
                    start_col,
                    start_byte,
                    self.current_byte_pos(),
                );
                self.diagnostics.push(Diagnostic::error(
                    "E0001",
                    format!("unexpected character `{}`", ch),
                    span,
                ));
                TokenKind::Ident(ch.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lexer_keywords_and_driver() {
        let src = r#"
struct Driver {
    id: i64
    name: String
    balance: i64
}

fn add_balance(driver: write Driver, amount: i64) {
    driver.balance += amount
}
"#;
        let mut lexer = Lexer::new("test.aps", src);
        let tokens = lexer.tokenize();
        assert!(!lexer.diagnostics.has_errors());
        assert_eq!(tokens[0].kind, TokenKind::Struct);
        assert_eq!(tokens[1].kind, TokenKind::Ident("Driver".to_string()));
        assert_eq!(tokens[2].kind, TokenKind::LBrace);
    }

    #[test]
    fn test_lexer_numbers_and_strings() {
        let src = r#"let a = 0xFF; let b = 0b1010; let c = 3.14; let s = "Hello\nAPS""#;
        let mut lexer = Lexer::new("test.aps", src);
        let tokens = lexer.tokenize();
        assert!(!lexer.diagnostics.has_errors());
        assert!(tokens.iter().any(|t| t.kind == TokenKind::IntLit(255, None)));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::IntLit(10, None)));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::StringLit("Hello\nAPS".to_string())));
    }
}
