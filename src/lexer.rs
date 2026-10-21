// Axiom lexer.
use crate::ast::Span;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    Fn,
    Struct,
    Let,
    If,
    Else,
    While,
    For,
    In,
    Return,
    Break,
    Continue,
    True,
    False,
    Use,
    Invariant,
    Requires,
    Ensures,
    Check,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    Dot,
    Arrow,
    Assign,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub fn lex(src: &str, file: &str) -> Result<Vec<Token>, String> {
    let b = src.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0usize;
    let mut line: u32 = 1;
    let err = |line: u32, msg: &str| format!("{}:{}: {}", file, line, msg);
    while i < b.len() {
        let c = b[i] as char;
        let start = i;
        let tline = line;
        let push = |toks: &mut Vec<Token>, tok: Tok, i: usize| {
            toks.push(Token {
                tok,
                span: Span { file: file.to_string(), line: tline, start, end: i },
            });
        };
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            ' ' | '\t' | '\r' => {
                i += 1;
            }
            '/' if i + 1 < b.len() && b[i + 1] == b'/' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            '/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                i += 2;
                let mut closed = false;
                while i + 1 < b.len() {
                    if b[i] == b'\n' {
                        line += 1;
                    }
                    if b[i] == b'*' && b[i + 1] == b'/' {
                        i += 2;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    return Err(err(tline, "unterminated block comment"));
                }
            }
            '0'..='9' => {
                let mut is_float = false;
                while i < b.len() {
                    let d = b[i] as char;
                    if d.is_ascii_digit() {
                        i += 1;
                    } else if d == '.' && i + 1 < b.len() && (b[i + 1] as char).is_ascii_digit() {
                        is_float = true;
                        i += 1;
                    } else if d == 'e' || d == 'E' {
                        is_float = true;
                        i += 1;
                        if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                            i += 1;
                        }
                    } else {
                        break;
                    }
                }
                let text = &src[start..i];
                if is_float {
                    let v: f64 = text.parse().map_err(|_| err(tline, "bad float literal"))?;
                    push(&mut toks, Tok::Float(v), i);
                } else {
                    let v: i64 = text.parse().map_err(|_| err(tline, "bad int literal"))?;
                    push(&mut toks, Tok::Int(v), i);
                }
            }
            '"' => {
                i += 1;
                let mut s = String::new();
                let mut closed = false;
                while i < b.len() {
                    let ch = b[i] as char;
                    if ch == '"' {
                        i += 1;
                        closed = true;
                        break;
                    } else if ch == '\\' {
                        i += 1;
                        if i >= b.len() {
                            break;
                        }
                        match b[i] as char {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            '0' => s.push('\0'),
                            '"' => s.push('"'),
                            '\\' => s.push('\\'),
                            other => return Err(err(line, &format!("bad escape \\{}", other))),
                        }
                        i += 1;
                    } else if ch == '\n' {
                        return Err(err(line, "newline in string literal"));
                    } else {
                        // UTF-8 safe: copy the whole char.
                        let ch_len = ch.len_utf8();
                        s.push_str(&src[i..i + ch_len]);
                        i += ch_len;
                    }
                }
                if !closed {
                    return Err(err(tline, "unterminated string literal"));
                }
                push(&mut toks, Tok::Str(s), i);
            }
            'A'..='Z' | 'a'..='z' | '_' => {
                while i < b.len() {
                    let d = b[i] as char;
                    if d.is_ascii_alphanumeric() || d == '_' {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let word = &src[start..i];
                let tok = match word {
                    "fn" => Tok::Fn,
                    "struct" => Tok::Struct,
                    "let" => Tok::Let,
                    "if" => Tok::If,
                    "else" => Tok::Else,
                    "while" => Tok::While,
                    "for" => Tok::For,
                    "in" => Tok::In,
                    "return" => Tok::Return,
                    "break" => Tok::Break,
                    "continue" => Tok::Continue,
                    "true" => Tok::True,
                    "false" => Tok::False,
                    "use" => Tok::Use,
                    "invariant" => Tok::Invariant,
                    "requires" => Tok::Requires,
                    "ensures" => Tok::Ensures,
                    "check" => Tok::Check,
                    _ => Tok::Ident(word.to_string()),
                };
                push(&mut toks, tok, i);
            }
            '(' => { i += 1; push(&mut toks, Tok::LParen, i); }
            ')' => { i += 1; push(&mut toks, Tok::RParen, i); }
            '{' => { i += 1; push(&mut toks, Tok::LBrace, i); }
            '}' => { i += 1; push(&mut toks, Tok::RBrace, i); }
            '[' => { i += 1; push(&mut toks, Tok::LBracket, i); }
            ']' => { i += 1; push(&mut toks, Tok::RBracket, i); }
            ',' => { i += 1; push(&mut toks, Tok::Comma, i); }
            ';' => { i += 1; push(&mut toks, Tok::Semi, i); }
            ':' => { i += 1; push(&mut toks, Tok::Colon, i); }
            '.' => { i += 1; push(&mut toks, Tok::Dot, i); }
            '+' => { i += 1; push(&mut toks, Tok::Plus, i); }
            '*' => { i += 1; push(&mut toks, Tok::Star, i); }
            '%' => { i += 1; push(&mut toks, Tok::Percent, i); }
            '/' => { i += 1; push(&mut toks, Tok::Slash, i); }
            '-' => {
                if i + 1 < b.len() && b[i + 1] == b'>' {
                    i += 2;
                    push(&mut toks, Tok::Arrow, i);
                } else {
                    i += 1;
                    push(&mut toks, Tok::Minus, i);
                }
            }
            '=' => {
                if i + 1 < b.len() && b[i + 1] == b'=' {
                    i += 2;
                    push(&mut toks, Tok::Eq, i);
                } else {
                    i += 1;
                    push(&mut toks, Tok::Assign, i);
                }
            }
            '!' => {
                if i + 1 < b.len() && b[i + 1] == b'=' {
                    i += 2;
                    push(&mut toks, Tok::Ne, i);
                } else {
                    i += 1;
                    push(&mut toks, Tok::Bang, i);
                }
            }
            '<' => {
                if i + 1 < b.len() && b[i + 1] == b'=' {
                    i += 2;
                    push(&mut toks, Tok::Le, i);
                } else {
                    i += 1;
                    push(&mut toks, Tok::Lt, i);
                }
            }
            '>' => {
                if i + 1 < b.len() && b[i + 1] == b'=' {
                    i += 2;
                    push(&mut toks, Tok::Ge, i);
                } else {
                    i += 1;
                    push(&mut toks, Tok::Gt, i);
                }
            }
            '&' => {
                if i + 1 < b.len() && b[i + 1] == b'&' {
                    i += 2;
                    push(&mut toks, Tok::AndAnd, i);
                } else {
                    return Err(err(line, "single '&' is not supported, use '&&'"));
                }
            }
            '|' => {
                if i + 1 < b.len() && b[i + 1] == b'|' {
                    i += 2;
                    push(&mut toks, Tok::OrOr, i);
                } else {
                    return Err(err(line, "single '|' is not supported, use '||'"));
                }
            }
            _ => return Err(err(line, &format!("unexpected character '{}'", c))),
        }
    }
    toks.push(Token {
        tok: Tok::Eof,
        span: Span { file: file.to_string(), line, start: src.len(), end: src.len() },
    });
    Ok(toks)
}
