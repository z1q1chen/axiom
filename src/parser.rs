// Axiom parser: tokens -> AST. Also collects `use` directives.
use crate::ast::*;
use crate::lexer::{Tok, Token};

pub struct ParseOut {
    pub items: Vec<Item>,
    pub uses: Vec<String>,
}

pub struct Parser<'a> {
    toks: Vec<Token>,
    pos: usize,
    src: &'a str,
    file: String,
}

impl<'a> Parser<'a> {
    pub fn new(toks: Vec<Token>, src: &'a str, file: &str) -> Self {
        Parser { toks, pos: 0, src, file: file.to_string() }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let idx = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[idx].tok
    }

    // `Name {` starts a struct literal only when the brace opens a field
    // list: `{}` or `{ field: ...`. Otherwise it is a block (if/while/for).
    fn struct_literal_ahead(&self) -> bool {
        if *self.peek() != Tok::LBrace {
            return false;
        }
        match self.peek_at(1) {
            Tok::RBrace => true,
            Tok::Ident(_) => *self.peek_at(2) == Tok::Colon,
            _ => false,
        }
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span.clone()
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn err<T>(&self, msg: &str) -> Result<T, String> {
        let s = &self.toks[self.pos].span;
        Err(format!("{}:{}: parse error: {}", s.file, s.line, msg))
    }

    fn expect(&mut self, tok: &Tok, what: &str) -> Result<Token, String> {
        if self.peek() == tok {
            Ok(self.bump())
        } else {
            self.err(&format!("expected {}, found {:?}", what, self.peek()))
        }
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == tok {
            self.bump();
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Result<(String, Span), String> {
        let t = self.bump();
        match t.tok {
            Tok::Ident(name) => Ok((name, t.span)),
            other => Err(format!(
                "{}:{}: parse error: expected identifier, found {:?}",
                t.span.file, t.span.line, other
            )),
        }
    }

    fn text_of(&self, span: &Span) -> String {
        self.src[span.start..span.end].trim().to_string()
    }

    fn merge(&self, a: &Span, b: &Span) -> Span {
        Span { file: self.file.clone(), line: a.line, start: a.start, end: b.end }
    }

    pub fn parse_program(&mut self) -> Result<ParseOut, String> {
        let mut items = Vec::new();
        let mut uses = Vec::new();
        while *self.peek() != Tok::Eof {
            match self.peek() {
                Tok::Use => {
                    self.bump();
                    let t = self.bump();
                    match t.tok {
                        Tok::Str(path) => {
                            self.expect(&Tok::Semi, "';'")?;
                            uses.push(path);
                        }
                        other => {
                            return Err(format!(
                                "{}:{}: parse error: expected string after use, found {:?}",
                                t.span.file, t.span.line, other
                            ))
                        }
                    }
                }
                Tok::Struct => items.push(Item::Struct(self.parse_struct()?)),
                Tok::Fn => items.push(Item::Fn(self.parse_fn(None)?)),
                Tok::Impl => items.extend(self.parse_impl()?),
                Tok::Let => {
                    let (name, ann, expr, span) = self.parse_let_parts()?;
                    items.push(Item::Global(name, ann, expr, span));
                }
                other => {
                    return self
                        .err(&format!("unexpected top-level token {:?}", other))
                }
            }
        }
        Ok(ParseOut { items, uses })
    }

    fn parse_type(&mut self) -> Result<TypeExpr, String> {
        if self.eat(&Tok::LBracket) {
            let inner = self.parse_type()?;
            self.expect(&Tok::RBracket, "']'")?;
            return Ok(TypeExpr::Array(Box::new(inner)));
        }
        if self.eat(&Tok::LBrace) {
            let key = self.parse_type()?;
            self.expect(&Tok::Colon, "':'")?;
            let val = self.parse_type()?;
            self.expect(&Tok::RBrace, "'}'")?;
            return Ok(TypeExpr::Map(Box::new(key), Box::new(val)));
        }
        let (name, _) = self.ident()?;
        Ok(match name.as_str() {
            "int" => TypeExpr::Int,
            "float" => TypeExpr::Float,
            "bool" => TypeExpr::Bool,
            "string" => TypeExpr::Str,
            "void" => TypeExpr::Void,
            _ => {
                if *self.peek() == Tok::Lt {
                    self.bump();
                    let mut args = vec![self.parse_type()?];
                    while self.eat(&Tok::Comma) {
                        args.push(self.parse_type()?);
                    }
                    self.expect(&Tok::Gt, "'>'")?;
                    TypeExpr::Generic(name, args)
                } else {
                    TypeExpr::Named(name)
                }
            }
        })
    }

    // Optional `<T, U>` type parameter list after a definition name.
    fn parse_type_params(&mut self) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        if *self.peek() == Tok::Lt {
            self.bump();
            loop {
                let (p, _) = self.ident()?;
                out.push(p);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(&Tok::Gt, "'>'")?;
        }
        Ok(out)
    }

    // Speculative `<type, ...>` type argument list. Restores the
    // token position and returns None when the shape does not fit,
    // so `a < b` keeps parsing as a comparison.
    fn try_type_args(&mut self) -> Option<Vec<TypeExpr>> {
        if *self.peek() != Tok::Lt {
            return None;
        }
        let save = self.pos;
        self.bump();
        let mut args = Vec::new();
        loop {
            match self.parse_type() {
                Ok(t) => args.push(t),
                Err(_) => {
                    self.pos = save;
                    return None;
                }
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        if !self.eat(&Tok::Gt) {
            self.pos = save;
            return None;
        }
        Some(args)
    }

    fn parse_struct(&mut self) -> Result<StructDef, String> {
        let start = self.expect(&Tok::Struct, "'struct'")?.span;
        let (name, _) = self.ident()?;
        let type_params = self.parse_type_params()?;
        self.expect(&Tok::LBrace, "'{'")?;
        let mut fields = Vec::new();
        let mut invariants = Vec::new();
        loop {
            match self.peek() {
                Tok::RBrace => {
                    let end = self.bump().span;
                    return Ok(StructDef {
                        name,
                        type_params,
                        fields,
                        invariants,
                        span: self.merge(&start, &end),
                    });
                }
                Tok::Invariant => {
                    self.bump();
                    let e = self.parse_expr()?;
                    let text = self.text_of(e.span());
                    let span = e.span().clone();
                    self.expect(&Tok::Semi, "';'")?;
                    invariants.push(Invariant { expr: e, text, span });
                }
                Tok::Ident(_) => {
                    let (fname, _) = self.ident()?;
                    self.expect(&Tok::Colon, "':'")?;
                    let ty = self.parse_type()?;
                    self.expect(&Tok::Semi, "';'")?;
                    fields.push((fname, ty));
                }
                other => {
                    return self.err(&format!(
                        "unexpected token in struct body: {:?}",
                        other
                    ))
                }
            }
        }
    }

    fn parse_impl(&mut self) -> Result<Vec<Item>, String> {
        self.expect(&Tok::Impl, "'impl'")?;
        let (name, _) = self.ident()?;
        self.expect(&Tok::LBrace, "'{'")?;
        let mut items = Vec::new();
        while *self.peek() == Tok::Fn {
            items.push(Item::Fn(self.parse_fn(Some(name.clone()))?));
        }
        self.expect(&Tok::RBrace, "'}'")?;
        Ok(items)
    }

    fn parse_fn(&mut self, owner: Option<String>) -> Result<FnDef, String> {
        let start = self.expect(&Tok::Fn, "'fn'")?.span;
        let (name, _) = self.ident()?;
        let type_params = self.parse_type_params()?;
        self.expect(&Tok::LParen, "'('")?;
        let mut params = Vec::new();
        if *self.peek() != Tok::RParen {
            loop {
                let (pname, _) = self.ident()?;
                // Inside an impl block, a bare `self` parameter takes
                // the impl's struct type without an annotation.
                let ty = if pname == "self" && owner.is_some()
                    && *self.peek() != Tok::Colon {
                    TypeExpr::Named(owner.clone().unwrap())
                } else {
                    self.expect(&Tok::Colon, "':'")?;
                    self.parse_type()?
                };
                params.push((pname, ty));
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(&Tok::RParen, "')'")?;
        let ret = if self.eat(&Tok::Arrow) {
            self.parse_type()?
        } else {
            TypeExpr::Void
        };
        self.expect(&Tok::LBrace, "'{'")?;
        // Leading contract clauses.
        let mut contracts = Vec::new();
        loop {
            let kind = match self.peek() {
                Tok::Requires => ContractKind::Requires,
                Tok::Ensures => ContractKind::Ensures,
                _ => break,
            };
            self.bump();
            let e = self.parse_expr()?;
            let text = self.text_of(e.span());
            let span = e.span().clone();
            self.expect(&Tok::Semi, "';'")?;
            contracts.push(Contract { kind, expr: e, text, span });
        }
        let body = self.parse_stmts_until_rbrace()?;
        Ok(FnDef { name, type_params, params, ret, contracts, body, span: start_span(&start, &self.file), owner })
    }

    fn parse_stmts_until_rbrace(&mut self) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        while *self.peek() != Tok::RBrace {
            if *self.peek() == Tok::Eof {
                return self.err("unexpected end of file inside block");
            }
            stmts.push(self.parse_stmt()?);
        }
        self.bump(); // consume '}'
        Ok(stmts)
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&Tok::LBrace, "'{'")?;
        self.parse_stmts_until_rbrace()
    }

    fn parse_let_parts(
        &mut self,
    ) -> Result<(String, Option<TypeExpr>, Expr, Span), String> {
        let start = self.expect(&Tok::Let, "'let'")?.span;
        let (name, _) = self.ident()?;
        let ann = if self.eat(&Tok::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&Tok::Assign, "'='")?;
        let expr = self.parse_expr()?;
        self.expect(&Tok::Semi, "';'")?;
        Ok((name, ann, expr, start))
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek() {
            Tok::Let => {
                let (name, ann, expr, span) = self.parse_let_parts()?;
                Ok(Stmt::Let(name, ann, expr, span))
            }
            Tok::If => {
                let span = self.bump().span;
                let cond = self.parse_expr()?;
                let then = self.parse_block()?;
                let els = if self.eat(&Tok::Else) {
                    if *self.peek() == Tok::If {
                        let inner = self.parse_stmt()?;
                        Some(vec![inner])
                    } else {
                        Some(self.parse_block()?)
                    }
                } else {
                    None
                };
                Ok(Stmt::If(cond, then, els, span))
            }
            Tok::While => {
                let span = self.bump().span;
                let cond = self.parse_expr()?;
                let body = self.parse_block()?;
                Ok(Stmt::While(cond, body, span))
            }
            Tok::For => {
                let span = self.bump().span;
                let (var, _) = self.ident()?;
                self.expect(&Tok::In, "'in'")?;
                let iter = self.parse_expr()?;
                let body = self.parse_block()?;
                Ok(Stmt::For(var, iter, body, span))
            }
            Tok::Return => {
                let span = self.bump().span;
                if self.eat(&Tok::Semi) {
                    Ok(Stmt::Return(None, span))
                } else {
                    let e = self.parse_expr()?;
                    self.expect(&Tok::Semi, "';'")?;
                    Ok(Stmt::Return(Some(e), span))
                }
            }
            Tok::Break => {
                let span = self.bump().span;
                self.expect(&Tok::Semi, "';'")?;
                Ok(Stmt::Break(span))
            }
            Tok::Continue => {
                let span = self.bump().span;
                self.expect(&Tok::Semi, "';'")?;
                Ok(Stmt::Continue(span))
            }
            Tok::Check => {
                self.bump();
                let e = self.parse_expr()?;
                let text = self.text_of(e.span());
                let span = e.span().clone();
                self.expect(&Tok::Semi, "';'")?;
                Ok(Stmt::Check(e, text, span))
            }
            Tok::Requires | Tok::Ensures => self.err(
                "contracts are only allowed at the top of a function body",
            ),
            _ => {
                // Expression statement or assignment.
                let e = self.parse_expr()?;
                if self.eat(&Tok::Assign) {
                    let value = self.parse_expr()?;
                    let end = self.expect(&Tok::Semi, "';'")?.span;
                    let span = self.merge(e.span(), &end);
                    return match e {
                        Expr::Ident(name, _) => Ok(Stmt::Assign(name, value, span)),
                        Expr::Field(obj, fname, _) => {
                            Ok(Stmt::FieldAssign(*obj, fname, value, span))
                        }
                        Expr::Index(arr, idx, _) => {
                            Ok(Stmt::IndexAssign(*arr, *idx, value, span))
                        }
                        _ => Err(format!(
                            "{}:{}: parse error: invalid assignment target",
                            span.file, span.line
                        )),
                    };
                }
                let span = e.span().clone();
                self.expect(&Tok::Semi, "';'")?;
                Ok(Stmt::Expr(e, span))
            }
        }
    }

    pub fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_and()?;
        while *self.peek() == Tok::OrOr {
            self.bump();
            let rhs = self.parse_and()?;
            let span = self.merge(lhs.span(), rhs.span());
            lhs = Expr::Bin(BinOp::Or, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_eq()?;
        while *self.peek() == Tok::AndAnd {
            self.bump();
            let rhs = self.parse_eq()?;
            let span = self.merge(lhs.span(), rhs.span());
            lhs = Expr::Bin(BinOp::And, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn parse_eq(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_cmp()?;
        loop {
            let op = match self.peek() {
                Tok::Eq => BinOp::Eq,
                Tok::Ne => BinOp::Ne,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_cmp()?;
            let span = self.merge(lhs.span(), rhs.span());
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn parse_cmp(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek() {
                Tok::Lt => BinOp::Lt,
                Tok::Le => BinOp::Le,
                Tok::Gt => BinOp::Gt,
                Tok::Ge => BinOp::Ge,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_add()?;
            let span = self.merge(lhs.span(), rhs.span());
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_mul()?;
            let span = self.merge(lhs.span(), rhs.span());
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_unary()?;
            let span = self.merge(lhs.span(), rhs.span());
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Tok::Minus => {
                let span = self.bump().span;
                let e = self.parse_unary()?;
                let span = self.merge(&span, e.span());
                Ok(Expr::Un(UnOp::Neg, Box::new(e), span))
            }
            Tok::Bang => {
                let span = self.bump().span;
                let e = self.parse_unary()?;
                let span = self.merge(&span, e.span());
                Ok(Expr::Un(UnOp::Not, Box::new(e), span))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_primary()?;
        loop {
            match self.peek() {
                Tok::LParen => {
                    self.bump();
                    let mut args = Vec::new();
                    if *self.peek() != Tok::RParen {
                        loop {
                            args.push(self.parse_expr()?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                    }
                    let end = self.expect(&Tok::RParen, "')'")?.span;
                    let span = self.merge(e.span(), &end);
                    e = Expr::Call(Box::new(e), args, span);
                }
                Tok::Dot => {
                    self.bump();
                    let (fname, fspan) = self.ident()?;
                    let span = self.merge(e.span(), &fspan);
                    e = Expr::Field(Box::new(e), fname, span);
                }
                Tok::LBracket => {
                    self.bump();
                    let idx = self.parse_expr()?;
                    let end = self.expect(&Tok::RBracket, "']'")?.span;
                    let span = self.merge(e.span(), &end);
                    e = Expr::Index(Box::new(e), Box::new(idx), span);
                }
                _ => break,
            }
        }
        Ok(e)
    }

    // String interpolation: "{expr}" inside a string literal is
    // desugared to string concatenation with str(expr). "{{" and
    // "}}" are literal braces. A lone "}" is an error. Strings
    // without braces take the fast path and stay plain literals.
    fn interpolate(&mut self, s: String, span: Span) -> Result<Expr, String> {
        if !s.contains('{') && !s.contains('}') {
            return Ok(Expr::Str(s, span));
        }
        fn utf8_len(b: u8) -> usize {
            if b < 0x80 { 1 } else if b >= 0xF0 { 4 } else if b >= 0xE0 { 3 } else { 2 }
        }
        let bytes = s.as_bytes();
        let mut parts: Vec<Expr> = Vec::new();
        let mut lit = String::new();
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                lit.push('{');
                i += 2;
                continue;
            }
            if b == b'}' && i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                lit.push('}');
                i += 2;
                continue;
            }
            if b == b'{' {
                let start = i + 1;
                let mut depth = 1i32;
                let mut j = start;
                let mut in_str = false;
                while j < bytes.len() {
                    let c = bytes[j];
                    if in_str {
                        if c == b'"' { in_str = false; }
                    } else if c == b'"' {
                        in_str = true;
                    } else if c == b'{' {
                        depth += 1;
                    } else if c == b'}' {
                        depth -= 1;
                        if depth == 0 { break; }
                    }
                    j += 1;
                }
                if depth != 0 {
                    return Err(format!(
                        "{}:{}: parse error: unterminated '{{' in string interpolation",
                        span.file, span.line
                    ));
                }
                let inner = s[start..j].to_string();
                if !lit.is_empty() {
                    parts.push(Expr::Str(std::mem::take(&mut lit), span.clone()));
                }
                let toks = crate::lexer::lex(&inner, &span.file)?;
                let mut sub = Parser::new(toks, &inner, &span.file);
                let e = sub.parse_expr()?;
                if *sub.peek() != Tok::Eof {
                    return Err(format!(
                        "{}:{}: parse error: trailing tokens in interpolation",
                        span.file, span.line
                    ));
                }
                parts.push(Expr::Call(
                    Box::new(Expr::Ident("str".to_string(), span.clone())),
                    vec![e],
                    span.clone(),
                ));
                i = j + 1;
                continue;
            }
            if b == b'}' {
                return Err(format!(
                    "{}:{}: parse error: unmatched '}}' in string; write '}}}}' for a literal brace",
                    span.file, span.line
                ));
            }
            let n = utf8_len(b);
            lit.push_str(&s[i..i + n]);
            i += n;
        }
        if !lit.is_empty() {
            parts.push(Expr::Str(lit, span.clone()));
        }
        let mut iter = parts.into_iter();
        let mut acc = match iter.next() {
            Some(e) => e,
            None => return Ok(Expr::Str(String::new(), span)),
        };
        for p in iter {
            acc = Expr::Bin(BinOp::Add, Box::new(acc), Box::new(p), span.clone());
        }
        Ok(acc)
    }

    // The field list of a struct literal, braces included.
    fn parse_struct_fields(&mut self) -> Result<(Vec<(String, Expr)>, Span), String> {
        self.bump(); // '{'
        let mut fields = Vec::new();
        if *self.peek() != Tok::RBrace {
            loop {
                let (fname, _) = self.ident()?;
                self.expect(&Tok::Colon, "':'")?;
                let fe = self.parse_expr()?;
                fields.push((fname, fe));
                if !self.eat(&Tok::Comma) {
                    break;
                }
                if *self.peek() == Tok::RBrace {
                    break;
                }
            }
        }
        let end = self.expect(&Tok::RBrace, "'}'")?.span;
        Ok((fields, end))
    }

    // `Name<type, ...>` followed by a struct literal or a call.
    // Returns None (with the position restored) when the tokens do
    // not have that shape, so comparisons still parse normally.
    fn try_generic_syntax(
        &mut self,
        name: &str,
        start: &Span,
    ) -> Result<Option<Expr>, String> {
        let save = self.pos;
        let targs = match self.try_type_args() {
            Some(a) => a,
            None => return Ok(None),
        };
        if self.struct_literal_ahead() {
            let (fields, end) = self.parse_struct_fields()?;
            let span = self.merge(start, &end);
            return Ok(Some(Expr::StructLit(
                name.to_string(),
                targs,
                fields,
                span,
            )));
        }
        if *self.peek() == Tok::LParen {
            self.bump();
            let mut args = Vec::new();
            if *self.peek() != Tok::RParen {
                loop {
                    args.push(self.parse_expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
            }
            let end = self.expect(&Tok::RParen, "')'")?.span;
            let span = self.merge(start, &end);
            return Ok(Some(Expr::GenericCall(
                name.to_string(),
                targs,
                args,
                span,
            )));
        }
        self.pos = save;
        Ok(None)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        let t = self.bump();
        match t.tok {
            Tok::Int(v) => Ok(Expr::Int(v, t.span)),
            Tok::Float(v) => Ok(Expr::Float(v, t.span)),
            Tok::Str(s) => self.interpolate(s, t.span),
            Tok::True => Ok(Expr::Bool(true, t.span)),
            Tok::False => Ok(Expr::Bool(false, t.span)),
            Tok::Ident(name) => {
                if self.struct_literal_ahead() {
                    let (fields, end) = self.parse_struct_fields()?;
                    let span = self.merge(&t.span, &end);
                    Ok(Expr::StructLit(name, Vec::new(), fields, span))
                } else if let Some(e) = self.try_generic_syntax(&name, &t.span)? {
                    Ok(e)
                } else {
                    Ok(Expr::Ident(name, t.span))
                }
            }
            Tok::LParen => {
                let e = self.parse_expr()?;
                self.expect(&Tok::RParen, "')'")?;
                Ok(e)
            }
            Tok::LBracket => {
                let mut elems = Vec::new();
                if *self.peek() != Tok::RBracket {
                    loop {
                        elems.push(self.parse_expr()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                        if *self.peek() == Tok::RBracket {
                            break;
                        }
                    }
                }
                let end = self.expect(&Tok::RBracket, "']'")?.span;
                let span = self.merge(&t.span, &end);
                Ok(Expr::Array(elems, span))
            }
            Tok::LBrace => {
                // Map literal: { k: v, ... }
                let mut pairs = Vec::new();
                if *self.peek() != Tok::RBrace {
                    loop {
                        let k = self.parse_expr()?;
                        self.expect(&Tok::Colon, "':'")?;
                        let v = self.parse_expr()?;
                        pairs.push((k, v));
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                        if *self.peek() == Tok::RBrace {
                            break;
                        }
                    }
                }
                let end = self.expect(&Tok::RBrace, "'}'")?.span;
                let span = self.merge(&t.span, &end);
                Ok(Expr::MapLit(pairs, span))
            }
            other => Err(format!(
                "{}:{}: parse error: unexpected token {:?}",
                t.span.file, t.span.line, other
            )),
        }
    }
}

fn start_span(s: &Span, file: &str) -> Span {
    Span { file: file.to_string(), line: s.line, start: s.start, end: s.end }
}
