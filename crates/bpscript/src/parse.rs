use crate::ast::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Quoted(String),
    Int(String, String),
    Float(String),
    Str(Option<char>, String),
    Hash(i64),
    Punct(&'static str),
    Eof,
}

#[derive(Debug, thiserror::Error)]
#[error("line {line}: {msg}")]
pub struct ParseError {
    pub line: usize,
    pub msg: String,
}

pub type PResult<T> = std::result::Result<T, ParseError>;

const PUNCTS: &[&str] = &[
    "::", "?.", "@.", "->", "+=", "-=", "=>", "(", ")", "{", "}", "[", "]", "<", ">", ",", ";",
    ":", ".", "=", "!", "*", "@", "_",
];

fn lex(src: &str) -> PResult<Vec<(Tok, usize)>> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;
    let err = |line: usize, msg: &str| ParseError {
        line,
        msg: msg.to_string(),
    };
    let read_quoted = |i: &mut usize, line: &mut usize, q: char| -> PResult<String> {
        let mut s = String::new();
        *i += 1;
        loop {
            let c = *chars
                .get(*i)
                .ok_or_else(|| err(*line, "unterminated quote"))?;
            *i += 1;
            match c {
                '\\' => {
                    let e = *chars.get(*i).ok_or_else(|| err(*line, "bad escape"))?;
                    *i += 1;
                    match e {
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        'u' => {
                            if chars.get(*i) != Some(&'{') {
                                return Err(err(*line, "bad \\u escape"));
                            }
                            let end = chars[*i..]
                                .iter()
                                .position(|&c| c == '}')
                                .ok_or_else(|| err(*line, "bad \\u escape"))?;
                            let hex: String = chars[*i + 1..*i + end].iter().collect();
                            let v = u32::from_str_radix(&hex, 16)
                                .ok()
                                .and_then(char::from_u32)
                                .ok_or_else(|| err(*line, "bad \\u escape"))?;
                            s.push(v);
                            *i += end + 1;
                        }
                        c => s.push(c),
                    }
                }
                '\n' => {
                    *line += 1;
                    s.push('\n');
                }
                c if c == q => return Ok(s),
                c => s.push(c),
            }
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let start_line = line;
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == '"' {
            let s = read_quoted(&mut i, &mut line, '"')?;
            out.push((Tok::Str(None, s), start_line));
            continue;
        }
        if c == '\'' {
            let s = read_quoted(&mut i, &mut line, '\'')?;
            out.push((Tok::Quoted(s), start_line));
            continue;
        }
        if c == '`' {
            let s = read_quoted(&mut i, &mut line, '`')?;
            out.push((Tok::Ident(format!("`{s}")), start_line));
            continue;
        }
        if c == '#' {
            let mut j = i + 1;
            if chars.get(j) == Some(&'-') {
                j += 1;
            }
            let st = j;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if st == j {
                return Err(err(line, "expected a number after #"));
            }
            let text: String = chars[i + 1..j].iter().collect();
            let v = text.parse().map_err(|_| err(line, "bad number"))?;
            out.push((Tok::Hash(v), start_line));
            i = j;
            continue;
        }
        let neg_num = c == '-' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit());
        if c.is_ascii_digit() || neg_num {
            let mut j = i + 1;
            if c == '0' && matches!(chars.get(j), Some('x' | 'X')) {
                j += 1;
                while j < chars.len() && chars[j].is_ascii_hexdigit() {
                    j += 1;
                }
                let text: String = chars[i..j].iter().collect();
                out.push((Tok::Int(text, String::new()), start_line));
                i = j;
                continue;
            }
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            let mut is_float = false;
            if chars.get(j) == Some(&'.') && chars.get(j + 1).is_some_and(|d| d.is_ascii_digit()) {
                is_float = true;
                j += 1;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
            }
            if matches!(chars.get(j), Some('e' | 'E')) {
                let mut k = j + 1;
                if matches!(chars.get(k), Some('+' | '-')) {
                    k += 1;
                }
                if chars.get(k).is_some_and(|d| d.is_ascii_digit()) {
                    is_float = true;
                    j = k;
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                }
            }
            let text: String = chars[i..j].iter().collect();
            if is_float {
                out.push((Tok::Float(text), start_line));
                i = j;
                continue;
            }
            let mut k = j;
            while k < chars.len() && chars[k].is_ascii_alphanumeric() {
                k += 1;
            }
            let suffix: String = chars[j..k].iter().collect();
            out.push((Tok::Int(text, suffix), start_line));
            i = k;
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let text: String = chars[i..j].iter().collect();
            if matches!(text.as_str(), "n" | "a" | "u") && chars.get(j) == Some(&'"') {
                i = j;
                let s = read_quoted(&mut i, &mut line, '"')?;
                out.push((Tok::Str(text.chars().next(), s), start_line));
                continue;
            }
            if text == "_" {
                out.push((Tok::Punct("_"), start_line));
            } else {
                out.push((Tok::Ident(text), start_line));
            }
            i = j;
            continue;
        }
        let rest: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        let p = PUNCTS
            .iter()
            .find(|p| rest.starts_with(**p))
            .ok_or_else(|| err(line, &format!("unexpected character {c:?}")))?;
        out.push((Tok::Punct(p), start_line));
        i += p.chars().count();
    }
    out.push((Tok::Eof, line));
    Ok(out)
}

pub struct Parser {
    toks: Vec<(Tok, usize)>,
    pos: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Function(Function),
    Import(Ref),
    Patch(crate::patch::PatchItem),
}

impl Parser {
    pub fn new(src: &str) -> PResult<Self> {
        Ok(Parser {
            toks: lex(src)?,
            pos: 0,
        })
    }

    pub fn peek(&self) -> &Tok {
        &self.toks[self.pos].0
    }

    pub fn peek_at(&self, k: usize) -> &Tok {
        &self.toks[(self.pos + k).min(self.toks.len() - 1)].0
    }

    pub fn line(&self) -> usize {
        self.toks[self.pos].1
    }

    pub fn err<T>(&self, msg: impl Into<String>) -> PResult<T> {
        Err(ParseError {
            line: self.line(),
            msg: msg.into(),
        })
    }

    pub fn bump(&mut self) -> Tok {
        let t = self.toks[self.pos].0.clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    pub fn is_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::Punct(x) if *x == p)
    }

    pub fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Ident(x) if x == k)
    }

    pub fn eat(&mut self, p: &str) -> bool {
        if self.is_punct(p) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub fn eat_kw(&mut self, k: &str) -> bool {
        if self.is_kw(k) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub fn expect(&mut self, p: &str) -> PResult<()> {
        if self.eat(p) {
            Ok(())
        } else {
            self.err(format!("expected '{p}', found {}", describe(self.peek())))
        }
    }

    pub fn expect_kw(&mut self, k: &str) -> PResult<()> {
        if self.eat_kw(k) {
            Ok(())
        } else {
            self.err(format!("expected '{k}', found {}", describe(self.peek())))
        }
    }

    pub fn at_eof(&self) -> bool {
        matches!(self.peek(), Tok::Eof)
    }

    pub fn name(&mut self) -> PResult<Name> {
        let text = match self.peek().clone() {
            Tok::Ident(s) => {
                if let Some(q) = s.strip_prefix('`') {
                    q.to_string()
                } else if KEYWORDS.contains(&s.as_str()) {
                    return self.err(format!("'{s}' is a keyword, write it as `{s}`"));
                } else {
                    s
                }
            }
            t => return self.err(format!("expected a name, found {}", describe(&t))),
        };
        self.bump();
        let number = match self.peek() {
            Tok::Hash(n) => {
                let n = *n as i32;
                self.bump();
                Some(n)
            }
            _ => None,
        };
        Ok(Name { text, number })
    }

    pub fn ident(&mut self) -> PResult<String> {
        match self.bump() {
            Tok::Ident(s) if !s.starts_with('`') => Ok(s),
            t => {
                self.pos -= 1;
                self.err(format!("expected a name, found {}", describe(&t)))
            }
        }
    }

    fn is_name_start(&self) -> bool {
        match self.peek() {
            Tok::Ident(s) => s.starts_with('`') || !KEYWORDS.contains(&s.as_str()),
            _ => false,
        }
    }

    pub fn int(&mut self) -> PResult<i64> {
        match self.bump() {
            Tok::Int(t, s) if s.is_empty() => parse_int(&t).ok_or(()).or_else(|_| {
                self.pos -= 1;
                self.err("bad number")
            }),
            t => {
                self.pos -= 1;
                self.err(format!("expected a number, found {}", describe(&t)))
            }
        }
    }

    fn ref_head(&mut self) -> PResult<Option<Ref>> {
        match self.peek().clone() {
            Tok::Quoted(s) => {
                self.bump();
                Ok(Some(Ref::Quoted {
                    class: None,
                    path: s,
                }))
            }
            Tok::Hash(n) => {
                self.bump();
                Ok(Some(Ref::Index(n as i32)))
            }
            Tok::Ident(_) if matches!(self.peek_at(1), Tok::Quoted(_)) && self.is_name_start() => {
                let class = self.name()?;
                let Tok::Quoted(s) = self.bump() else {
                    unreachable!()
                };
                Ok(Some(Ref::Quoted {
                    class: Some(class),
                    path: s,
                }))
            }
            _ => Ok(None),
        }
    }

    pub fn reference(&mut self) -> PResult<Ref> {
        if let Some(r) = self.ref_head()? {
            if self.is_punct("::") {
                return self.err("a quoted path cannot be followed by '::' here");
            }
            return Ok(r);
        }
        let mut segs = vec![self.name()?];
        while self.eat("::") {
            segs.push(self.name()?);
        }
        Ok(Ref::Path(segs))
    }

    pub fn ptr(&mut self) -> PResult<Ptr> {
        if self.eat_kw("none") {
            return Ok(Ptr::null());
        }
        let (owner, mut segs) = match self.ref_head()? {
            Some(r) => {
                self.expect("::")?;
                (Some(r), vec![])
            }
            None => (None, vec![]),
        };
        if owner.is_some() && self.is_punct("{") {
            return Ok(Ptr {
                owner,
                path: self.brace_names()?,
            });
        }
        if owner.is_none() && self.is_punct("{") {
            return Ok(Ptr {
                owner: None,
                path: self.brace_names()?,
            });
        }
        segs.push(self.name()?);
        while self.eat("::") {
            if self.is_punct("{") {
                let owner = Some(Ref::Path(segs));
                return Ok(Ptr {
                    owner,
                    path: self.brace_names()?,
                });
            }
            segs.push(self.name()?);
        }
        let last = segs.pop().expect("one segment");
        let owner = match owner {
            Some(o) => Some(o),
            None if segs.is_empty() => None,
            None => Some(Ref::Path(segs)),
        };
        Ok(Ptr {
            owner,
            path: vec![last],
        })
    }

    fn brace_names(&mut self) -> PResult<Vec<Name>> {
        self.expect("{")?;
        let mut v = vec![];
        while !self.eat("}") {
            v.push(self.name()?);
            if !self.is_punct("}") {
                self.expect(",")?;
            }
        }
        Ok(v)
    }

    fn target(&mut self) -> PResult<Target> {
        match self.peek().clone() {
            Tok::Int(t, s) if s.is_empty() => {
                self.bump();
                parse_int(&t)
                    .and_then(|v| u32::try_from(v).ok())
                    .map(Target::Raw)
                    .ok_or(())
                    .or_else(|_| self.err("bad offset"))
            }
            Tok::Ident(s) if !s.starts_with('`') && !KEYWORDS.contains(&s.as_str()) => {
                self.bump();
                Ok(Target::Label(s))
            }
            t => self.err(format!("expected a label, found {}", describe(&t))),
        }
    }

    pub fn items(&mut self) -> PResult<Vec<Item>> {
        let mut out = vec![];
        while !self.at_eof() {
            if let Some(p) = crate::patch::parse_item(self)? {
                out.push(Item::Patch(p));
            } else if self.eat_kw("function") {
                out.push(Item::Function(self.function_rest()?));
            } else if self.eat_kw("import") {
                let r = self.reference()?;
                self.expect(";")?;
                out.push(Item::Import(r));
            } else {
                return self.err(format!("unexpected {}", describe(self.peek())));
            }
        }
        Ok(out)
    }

    pub fn function_rest(&mut self) -> PResult<Function> {
        let name = self.reference()?;
        let noend = self.eat_kw("noend");
        let body = self.block()?;
        Ok(Function { name, noend, body })
    }

    pub fn block(&mut self) -> PResult<Vec<Stmt>> {
        self.expect("{")?;
        let mut body = vec![];
        while !self.eat("}") {
            if self.at_eof() {
                return self.err("missing '}'");
            }
            body.push(self.stmt()?);
        }
        Ok(body)
    }

    pub fn stmt(&mut self) -> PResult<Stmt> {
        if let Tok::Ident(s) = self.peek().clone() {
            if !s.starts_with('`') && self.peek_at(1) == &Tok::Punct(":") {
                self.bump();
                self.bump();
                return Ok(Stmt::Label(s));
            }
        }
        if self.eat_kw("clear") {
            let e = self.expr()?;
            self.expect(";")?;
            return Ok(Stmt::Expr(Expr::ClearMulti(Box::new(e))));
        }
        let s = self.stmt_body()?;
        if let Stmt::Expr(e) = &s {
            if self.eat("+=") {
                let b = self.expr()?;
                self.expect(";")?;
                return Ok(Stmt::Expr(Expr::AddMulti(Box::new(e.clone()), Box::new(b))));
            }
            if self.eat("-=") {
                let b = self.expr()?;
                self.expect(";")?;
                return Ok(Stmt::Expr(Expr::RemoveMulti(
                    Box::new(e.clone()),
                    Box::new(b),
                )));
            }
        }
        self.expect(";")?;
        Ok(s)
    }

    fn stmt_body(&mut self) -> PResult<Stmt> {
        if self.eat_kw("goto") {
            if self.eat("*") {
                return Ok(Stmt::GotoComputed(self.expr()?));
            }
            return Ok(Stmt::Goto(self.target()?));
        }
        if self.eat_kw("if") {
            self.expect("!")?;
            let c = self.expr()?;
            self.expect_kw("goto")?;
            return Ok(Stmt::IfNot(c, self.target()?));
        }
        if self.eat_kw("push") {
            return Ok(Stmt::Push(self.target()?));
        }
        if self.eat_kw("pop") {
            if self.eat_kw("if") {
                self.expect("!")?;
                return Ok(Stmt::PopIfNot(self.expr()?));
            }
            return Ok(Stmt::Pop);
        }
        if self.eat_kw("return") {
            if self.is_punct(";") || self.is_punct("}") {
                return Ok(Stmt::Return(None));
            }
            return Ok(Stmt::Return(Some(self.expr()?)));
        }
        if self.eat_kw("nop") {
            return Ok(Stmt::Nop);
        }
        if self.eat_kw("end") {
            return Ok(Stmt::End);
        }
        let kw = match self.peek() {
            Tok::Ident(s) => match s.as_str() {
                "let" => Some(LetKw::Let),
                "letbool" => Some(LetKw::Bool),
                "letobj" => Some(LetKw::Obj),
                "letweak" => Some(LetKw::Weak),
                "letdelegate" => Some(LetKw::Delegate),
                "letmulticast" => Some(LetKw::Multicast),
                "letframe" => Some(LetKw::Frame),
                _ => None,
            },
            _ => None,
        };
        let mut ptr = None;
        if kw.is_some() {
            self.bump();
            if kw == Some(LetKw::Let) && self.eat("(") {
                ptr = Some(self.ptr()?);
                self.expect(")")?;
            }
        }
        let dest = self.expr()?;
        if kw.is_none() && !self.is_punct("=") {
            return Ok(Stmt::Expr(dest));
        }
        self.expect("=")?;
        if kw.is_none() {
            if self.is_punct("[") {
                return Ok(Stmt::SetArray(dest, self.list("[", "]")?));
            }
            if self.is_kw("set") && self.peek_at(1) == &Tok::Punct("[") {
                self.bump();
                return Ok(Stmt::SetSet(dest, self.list("[", "]")?));
            }
            if self.is_kw("map") && self.peek_at(1) == &Tok::Punct("[") {
                self.bump();
                return Ok(Stmt::SetMap(dest, self.pair_list(":")?));
            }
        }
        let value = self.expr()?;
        Ok(Stmt::Let {
            kw,
            ptr,
            dest: Box::new(dest),
            value: Box::new(value),
        })
    }

    fn list(&mut self, open: &str, close: &str) -> PResult<Vec<Expr>> {
        self.expect(open)?;
        let mut v = vec![];
        while !self.eat(close) {
            v.push(self.expr()?);
            if !self.is_punct(close) {
                self.expect(",")?;
            }
        }
        Ok(v)
    }

    fn pair_list(&mut self, sep: &str) -> PResult<Vec<(Expr, Expr)>> {
        self.expect("[")?;
        let mut v = vec![];
        while !self.eat("]") {
            let k = self.expr()?;
            self.expect(sep)?;
            v.push((k, self.expr()?));
            if !self.is_punct("]") {
                self.expect(",")?;
            }
        }
        Ok(v)
    }

    pub fn expr(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            let op = if self.eat(".") {
                CtxOp::Normal
            } else if self.eat("?.") {
                CtxOp::FailSilent
            } else if self.eat("@.") {
                CtxOp::Class
            } else if self.eat("->") {
                e = Expr::Member(Box::new(e), self.ptr()?);
                continue;
            } else if self.eat("[") {
                let i = self.expr()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(i));
                continue;
            } else {
                return Ok(e);
            };
            let (mut rv, mut skip) = (None, None);
            if self.eat("[") {
                while !self.eat("]") {
                    let key = self.ident()?;
                    self.expect("=")?;
                    match key.as_str() {
                        "rv" => rv = Some(self.ptr()?),
                        "skip" => skip = Some(self.int()? as u32),
                        _ => return self.err(format!("unknown annotation {key}")),
                    }
                    if !self.is_punct("]") {
                        self.expect(",")?;
                    }
                }
            }
            let inner = self.member_operand()?;
            e = Expr::Ctx {
                obj: Box::new(e),
                op,
                rv,
                skip,
                inner: Box::new(inner),
            };
        }
    }

    fn member_operand(&mut self) -> PResult<Expr> {
        self.primary()
    }

    fn generic_ref(&mut self) -> PResult<Ref> {
        self.expect("<")?;
        let r = self.reference()?;
        self.expect(">")?;
        Ok(r)
    }

    fn paren1(&mut self) -> PResult<Expr> {
        self.expect("(")?;
        let e = self.expr()?;
        self.expect(")")?;
        Ok(e)
    }

    fn float_args(&mut self, n: usize) -> PResult<Vec<u32>> {
        self.expect("(")?;
        let mut v = vec![];
        for i in 0..n {
            if i > 0 {
                self.expect(",")?;
            }
            v.push(self.float_bits()?);
        }
        self.expect(")")?;
        Ok(v)
    }

    fn float_bits(&mut self) -> PResult<u32> {
        match self.bump() {
            Tok::Float(t) => t
                .parse::<f32>()
                .map(f32::to_bits)
                .or_else(|_| self.err("bad float")),
            Tok::Int(t, s) if s.is_empty() => {
                let v: f32 = parse_int(&t)
                    .map(|v| v as f32)
                    .ok_or(())
                    .or_else(|_| self.err("bad float"))?;
                Ok(v.to_bits())
            }
            Tok::Ident(k) if k == "float" => {
                self.expect("(")?;
                let v = self.int()?;
                self.expect(")")?;
                Ok(v as u32)
            }
            t => {
                self.pos -= 1;
                self.err(format!("expected a float, found {}", describe(&t)))
            }
        }
    }

    fn primary(&mut self) -> PResult<Expr> {
        let t = self.peek().clone();
        match t {
            Tok::Int(text, suffix) => {
                self.bump();
                let v = parse_int(&text);
                let bad = || self.err::<Expr>("bad number");
                let lit = match suffix.as_str() {
                    "" => match v.and_then(|v| i32::try_from(v).ok()) {
                        Some(v) => Lit::Int(v),
                        None => return bad(),
                    },
                    "i64" => match text.parse::<i64>() {
                        Ok(v) => Lit::Int64(v),
                        Err(_) => return bad(),
                    },
                    "u64" => match text.parse::<u64>() {
                        Ok(v) => Lit::UInt64(v),
                        Err(_) => return bad(),
                    },
                    "u8" => match text.parse::<u8>() {
                        Ok(v) => Lit::Byte(v),
                        Err(_) => return bad(),
                    },
                    _ => return self.err(format!("unknown number suffix {suffix}")),
                };
                Ok(Expr::Lit(lit))
            }
            Tok::Float(_) => Ok(Expr::Lit(Lit::Float(self.float_bits()?))),
            Tok::Str(prefix, s) => {
                self.bump();
                Ok(Expr::Lit(match prefix {
                    None => Lit::Str(s),
                    Some('a') => Lit::AnsiStr(s),
                    Some('u') => Lit::UniStr(s),
                    _ => {
                        let number = match self.peek() {
                            Tok::Hash(n) => {
                                let n = *n as i32;
                                self.bump();
                                Some(n)
                            }
                            _ => None,
                        };
                        Lit::Name(Name { text: s, number })
                    }
                }))
            }
            Tok::Punct("@") => {
                self.bump();
                Ok(Expr::Addr(self.target()?))
            }
            Tok::Punct("(") => self.paren1(),
            Tok::Punct("{") => {
                self.bump();
                let s = self.stmt_body()?;
                self.expect("}")?;
                Ok(Expr::Stmt(Box::new(s)))
            }
            Tok::Ident(k) if !k.starts_with('`') && KEYWORDS.contains(&k.as_str()) => {
                self.bump();
                self.keyword_expr(&k)
            }
            _ => self.path_expr(),
        }
    }

    fn path_expr(&mut self) -> PResult<Expr> {
        let head = self.ref_head()?;
        let mut segs = vec![];
        let mut brace_path = None;
        if head.is_none() {
            segs.push(self.name()?);
        }
        while self.is_punct("::") {
            self.bump();
            if self.is_punct("{") {
                brace_path = Some(self.brace_names()?);
                break;
            }
            segs.push(self.name()?);
        }
        let as_ref = |head: Option<Ref>, segs: Vec<Name>| match head {
            Some(h) => h,
            None => Ref::Path(segs),
        };
        if let Some(path) = brace_path {
            let owner = Some(match head {
                Some(h) => h,
                None => Ref::Path(segs),
            });
            return Ok(Expr::Var(Ptr { owner, path }));
        }
        if self.is_punct("(") {
            if head.is_some() && !segs.is_empty() {
                return self.err("a call target cannot mix a quoted path and '::'");
            }
            let args = self.list("(", ")")?;
            return Ok(Expr::Call {
                kind: None,
                head: as_ref(head, segs),
                args,
            });
        }
        let struct_size = if self.is_punct("<")
            && matches!(self.peek_at(1), Tok::Int(..))
            && self.peek_at(2) == &Tok::Punct(">")
            && self.peek_at(3) == &Tok::Punct("{")
        {
            self.bump();
            let s = self.int()? as i32;
            self.expect(">")?;
            Some(s)
        } else {
            None
        };
        if self.is_punct("{") {
            if head.is_some() && !segs.is_empty() {
                return self.err("a struct type cannot mix a quoted path and '::'");
            }
            let fields = self.list("{", "}")?;
            return Ok(Expr::Struct {
                ty: as_ref(head, segs),
                size: struct_size,
                fields,
            });
        }
        match head {
            Some(h) if segs.is_empty() => Ok(Expr::Obj(h)),
            Some(h) => {
                let last = segs.pop().expect("segment");
                if !segs.is_empty() {
                    return self.err("a quoted owner takes exactly one property name");
                }
                Ok(Expr::Var(Ptr {
                    owner: Some(h),
                    path: vec![last],
                }))
            }
            None => {
                let last = segs.pop().expect("segment");
                let owner = if segs.is_empty() {
                    None
                } else {
                    Some(Ref::Path(segs))
                };
                Ok(Expr::Var(Ptr {
                    owner,
                    path: vec![last],
                }))
            }
        }
    }

    fn keyword_expr(&mut self, k: &str) -> PResult<Expr> {
        let lit = |l: Lit| Ok(Expr::Lit(l));
        match k {
            "self" => lit(Lit::SelfObj),
            "none" => lit(Lit::NoObject),
            "nointerface" => lit(Lit::NoInterface),
            "nothing" => lit(Lit::Nothing),
            "true" => lit(Lit::True),
            "false" => lit(Lit::False),
            "intzero" => lit(Lit::IntZero),
            "intone" => lit(Lit::IntOne),
            "emptytext" => Ok(Expr::Text(Box::new(Text::Empty))),
            "intbyte" => {
                self.expect("(")?;
                let v = self.int()?;
                self.expect(")")?;
                match u8::try_from(v) {
                    Ok(v) => lit(Lit::IntByte(v)),
                    Err(_) => self.err("intbyte takes 0..255"),
                }
            }
            "float" => {
                self.pos -= 1;
                lit(Lit::Float(self.float_bits()?))
            }
            "vec" => {
                let v = self.float_args(3)?;
                lit(Lit::Vector([v[0], v[1], v[2]]))
            }
            "rot" => {
                let v = self.float_args(3)?;
                lit(Lit::Rotator([v[0], v[1], v[2]]))
            }
            "transform" => {
                let v = self.float_args(10)?;
                let mut a = [0u32; 10];
                a.copy_from_slice(&v);
                lit(Lit::Transform(a))
            }
            "token" => {
                self.expect("(")?;
                let t = self.ident()?;
                self.expect(")")?;
                lit(Lit::Token(t))
            }
            "local" | "out" | "inst" | "default" | "sparse" => {
                let kind = match k {
                    "local" => VarKind::Local,
                    "out" => VarKind::Out,
                    "inst" => VarKind::Inst,
                    "default" => VarKind::Default,
                    _ => VarKind::Sparse,
                };
                Ok(Expr::KindVar(kind, self.ptr()?))
            }
            "math" | "final" | "localfinal" | "virtual" | "localvirtual" => {
                let kind = match k {
                    "math" => CallKind::Math,
                    "final" => CallKind::Final,
                    "localfinal" => CallKind::LocalFinal,
                    "virtual" => CallKind::Virtual,
                    _ => CallKind::LocalVirtual,
                };
                let head = if matches!(kind, CallKind::Virtual | CallKind::LocalVirtual) {
                    Ref::Path(vec![self.name()?])
                } else {
                    self.reference()?
                };
                let args = self.list("(", ")")?;
                Ok(Expr::Call {
                    kind: Some(kind),
                    head,
                    args,
                })
            }
            "obj" => {
                self.expect("(")?;
                let r = self.reference()?;
                self.expect(")")?;
                Ok(Expr::Obj(r))
            }
            "cast" | "metacast" | "objtoiface" | "crossiface" | "ifacetoobj" => {
                let kind = match k {
                    "cast" => CastKind::Dynamic,
                    "metacast" => CastKind::Meta,
                    "objtoiface" => CastKind::ObjToIface,
                    "crossiface" => CastKind::CrossIface,
                    _ => CastKind::IfaceToObj,
                };
                let c = self.generic_ref()?;
                let e = self.paren1()?;
                Ok(Expr::Cast(kind, c, Box::new(e)))
            }
            "primcast" => {
                self.expect("<")?;
                let t = self.ident()?;
                self.expect(">")?;
                let e = self.paren1()?;
                Ok(Expr::PrimCast(t, Box::new(e)))
            }
            "switch" => {
                let index = self.paren1()?;
                self.expect("{")?;
                let mut cases = vec![];
                loop {
                    if self.eat("_") {
                        self.expect("=>")?;
                        let d = self.expr()?;
                        self.eat(",");
                        self.expect("}")?;
                        return Ok(Expr::Switch {
                            index: Box::new(index),
                            cases,
                            default: Box::new(d),
                        });
                    }
                    let c = self.expr()?;
                    self.expect("=>")?;
                    let v = self.expr()?;
                    cases.push((c, v));
                    self.expect(",")?;
                }
            }
            "skipoffset" => {
                self.expect("(")?;
                let t = self.target()?;
                self.expect(")")?;
                Ok(Expr::SkipOffset(t))
            }
            "nsloctext" => {
                let a = self.list("(", ")")?;
                let [ns, key, src]: [Expr; 3] = a
                    .try_into()
                    .or_else(|_| self.err("nsloctext takes 3 arguments"))?;
                Ok(Expr::Text(Box::new(Text::Localized(ns, key, src))))
            }
            "invtext" => Ok(Expr::Text(Box::new(Text::Invariant(self.paren1()?)))),
            "text" => Ok(Expr::Text(Box::new(Text::Literal(self.paren1()?)))),
            "loctable" => {
                self.expect("(")?;
                let r = self.reference()?;
                self.expect(",")?;
                let id = self.expr()?;
                self.expect(",")?;
                let key = self.expr()?;
                self.expect(")")?;
                Ok(Expr::Text(Box::new(Text::Table(r, id, key))))
            }
            "bind" => {
                self.expect("(")?;
                let d = self.expr()?;
                self.expect(",")?;
                let o = self.expr()?;
                self.expect(",")?;
                let n = self.name()?;
                self.expect(")")?;
                Ok(Expr::Bind(Box::new(d), Box::new(o), n))
            }
            "addmulti" | "removemulti" => {
                let a = self.list("(", ")")?;
                let [x, y]: [Expr; 2] = a
                    .try_into()
                    .or_else(|_| self.err(format!("{k} takes 2 arguments")))?;
                Ok(if k == "addmulti" {
                    Expr::AddMulti(Box::new(x), Box::new(y))
                } else {
                    Expr::RemoveMulti(Box::new(x), Box::new(y))
                })
            }
            "clearmulti" => Ok(Expr::ClearMulti(Box::new(self.paren1()?))),
            "broadcast" => {
                let sig = if self.is_punct("<") {
                    Some(self.generic_ref()?)
                } else {
                    None
                };
                let mut a = self.list("(", ")")?;
                if a.is_empty() {
                    return self.err("broadcast needs the delegate");
                }
                let del = a.remove(0);
                Ok(Expr::Broadcast {
                    sig,
                    del: Box::new(del),
                    args: a,
                })
            }
            "assert" => {
                self.expect("(")?;
                let e = self.expr()?;
                self.expect(",")?;
                let line = self.int()? as u16;
                self.expect(",")?;
                let dbg = if self.eat_kw("true") {
                    true
                } else {
                    self.expect_kw("false")?;
                    false
                };
                self.expect(")")?;
                Ok(Expr::Assert(Box::new(e), line, dbg))
            }
            "instrument" => {
                self.expect("(")?;
                let t = self.ident()?;
                let n = if self.eat(",") {
                    Some(self.name()?)
                } else {
                    None
                };
                self.expect(")")?;
                Ok(Expr::Instrument(t, n))
            }
            "fieldpath" => Ok(Expr::FieldPath(Box::new(self.paren1()?))),
            "softobj" => Ok(Expr::SoftObj(Box::new(self.paren1()?))),
            "interface" => Ok(Expr::Interface(Box::new(self.paren1()?))),
            "skip" => Ok(Expr::Skip(Box::new(self.paren1()?))),
            "property" => {
                self.expect("(")?;
                let p = self.ptr()?;
                self.expect(")")?;
                Ok(Expr::Property(p))
            }
            "delegate" => {
                self.expect("(")?;
                let n = self.name()?;
                self.expect(")")?;
                Ok(Expr::Delegate(n))
            }
            "arrayconst" | "setconst" => {
                self.expect("<")?;
                let p = self.ptr()?;
                self.expect(">")?;
                let v = self.list("[", "]")?;
                Ok(if k == "arrayconst" {
                    Expr::ArrayConst(p, v)
                } else {
                    Expr::SetConst(p, v)
                })
            }
            "mapconst" => {
                self.expect("<")?;
                let kp = self.ptr()?;
                self.expect(",")?;
                let vp = self.ptr()?;
                self.expect(">")?;
                let v = self.pair_list(":")?;
                Ok(Expr::MapConst(kp, vp, v))
            }
            _ => self.err(format!("'{k}' is a keyword, write it as `{k}`")),
        }
    }
}

pub fn parse_int(t: &str) -> Option<i64> {
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(h, 16).ok();
    }
    t.parse().ok()
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => format!("'{}'", s.trim_start_matches('`')),
        Tok::Quoted(s) => format!("'{s}'"),
        Tok::Int(t, s) => format!("{t}{s}"),
        Tok::Float(t) => t.clone(),
        Tok::Str(_, s) => format!("{s:?}"),
        Tok::Hash(n) => format!("#{n}"),
        Tok::Punct(p) => format!("'{p}'"),
        Tok::Eof => "end of file".into(),
    }
}

pub fn parse(src: &str) -> PResult<Vec<Item>> {
    Parser::new(src)?.items()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"function `Doesn't Want`::Foo noend {
    push L120;
    Count = 0;
    local Count = intzero;
    inst Owner::Value = intone;
    let(Owner::{Inner, Outer}) Arr[3] = intbyte(7);
    letbool bFlag = true;
    letframe ExecuteUbergraph_X::Copy = Param;
    x = Default__KismetSystemLibrary.PrintString(self, "a\"b\n", true, 1.5, -2, 7u8, 9i64, 10u64);
    x = thing.[rv=none, skip=13]math KismetMathLibrary::Not_PreBool(false);
    y = a?.b@.default C::d;
    z = cast<'/Game/A.B_C'>(none)->S::M;
    w = Margin<16>{0.1, 1e-7, -0.0, float(0x7fc00000)};
    v = switch(i) {0 => n"A_3", 1 => n"B"#4, _ => u"Ü"};
    t = nsloctext("ns", "key", "src");
    u = loctable(#-5, "id", "key");
    goto *EntryPoint;
L120:
    if !Cond goto L120;
    pop if !Cond;
    Del += Handler;
    Del -= Handler;
    clear Del;
    Arr = [1, 2];
    Set = set[1];
    Map = map[1: 2, 3: 4];
    broadcast<Sig>(Del, 1, 2);
    bind(Del, self, OnClick);
    ExecuteUbergraph_X(@L120);
    k = skipoffset(L120);
    k = skipoffset(77);
    q = arrayconst<A::{B, C}>[1];
    q = mapconst<K::k, V::v>[1: 2];
    q = vec(1.0, 2.0, 3.0);
    q = rot(1.0, 2.0, 3.0);
    q = { goto L120 };
    q = interface(x).virtual Foo#2();
    q = primcast<ObjectToBool>(x);
    q = token(EndParmValue);
    q = assert(x, 12, false);
    q = instrument(InlineEvent, Foo);
    q = fieldpath(x);
    q = property(none);
    q = softobj("/Game/X.X");
    q = delegate(Foo);
    q = skip(x);
    q = a"ansi";
    nop;
    end;
    goto 99;
    return x;
    return;
L999:
}
"##;

    fn functions(src: &str) -> Vec<Function> {
        parse(src)
            .unwrap()
            .into_iter()
            .map(|i| match i {
                Item::Function(f) => f,
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    }

    #[test]
    fn print_parse_is_stable() {
        let first = functions(SAMPLE);
        let text: String = first.iter().map(|f| f.to_string()).collect();
        let second = functions(&text);
        assert_eq!(first, second);
        let again: String = second.iter().map(|f| f.to_string()).collect();
        assert_eq!(text, again);
    }

    #[test]
    fn statements_parse_as_expected() {
        let f = &functions(SAMPLE)[0];
        assert!(f.noend);
        assert_eq!(f.name.to_string(), "`Doesn't Want`::Foo");
        assert!(matches!(&f.body[0], Stmt::Push(Target::Label(l)) if l == "L120"));
        match &f.body[1] {
            Stmt::Let {
                kw: None,
                ptr: None,
                dest,
                value,
            } => {
                assert!(matches!(**dest, Expr::Var(_)));
                assert_eq!(**value, Expr::Lit(Lit::Int(0)));
            }
            s => panic!("unexpected {s:?}"),
        }
        let ctx = f.body.iter().find_map(|s| match s {
            Stmt::Let { value, .. } => match value.as_ref() {
                Expr::Ctx {
                    rv, skip, inner, ..
                } if skip.is_some() => Some((rv.clone(), *skip, inner.clone())),
                _ => None,
            },
            _ => None,
        });
        let (rv, skip, inner) = ctx.expect("context");
        assert_eq!(rv, Some(Ptr::null()));
        assert_eq!(skip, Some(13));
        assert!(matches!(
            *inner,
            Expr::Call {
                kind: Some(CallKind::Math),
                ..
            }
        ));
        assert!(f
            .body
            .iter()
            .any(|s| matches!(s, Stmt::Expr(Expr::AddMulti(..)))));
        assert!(f
            .body
            .iter()
            .any(|s| matches!(s, Stmt::Expr(Expr::ClearMulti(..)))));
        assert!(matches!(f.body.last(), Some(Stmt::Label(l)) if l == "L999"));
    }

    #[test]
    fn floats_survive_text() {
        for v in [
            0.1f32,
            1e-7,
            -0.0,
            3.4028235e38,
            1.0,
            72.0,
            f32::MIN_POSITIVE,
            2.3,
        ] {
            let t = float_text(v.to_bits());
            let mut p = Parser::new(&t).unwrap();
            match p.expr().unwrap() {
                Expr::Lit(Lit::Float(b)) => assert_eq!(b, v.to_bits(), "{t}"),
                e => panic!("{t} parsed as {e:?}"),
            }
        }
        let nan = 0x7fc0_0001u32;
        let mut p = Parser::new(&float_text(nan)).unwrap();
        assert_eq!(p.expr().unwrap(), Expr::Lit(Lit::Float(nan)));
    }

    #[test]
    fn strings_and_names_escape() {
        for s in [
            "plain",
            "quote\" and \\ back",
            "tab\tnew\nline\r",
            "\u{1}ctl",
            "ümlaut ’",
        ] {
            let e = Expr::Lit(Lit::Str(s.into()));
            let mut p = Parser::new(&e.to_string()).unwrap();
            assert_eq!(p.expr().unwrap(), e);
        }
        for n in ["Plain", "With Space", "Doesn't", "self", "a`b", "9lives"] {
            let name = Name::new(n);
            let mut p = Parser::new(&name.to_string()).unwrap();
            assert_eq!(p.name().unwrap(), name);
        }
        let name = Name {
            text: "Foo".into(),
            number: Some(3),
        };
        let mut p = Parser::new(&name.to_string()).unwrap();
        assert_eq!(p.name().unwrap(), name);
    }

    #[test]
    fn errors_have_lines() {
        let e = parse("function Foo {\n    x = ;\n}\n").unwrap_err();
        assert_eq!(e.line, 2);
        assert!(parse("function Foo {\n    goto;\n}").is_err());
        assert!(parse("function Foo { self = 1 }").is_err());
    }

    #[test]
    fn patch_items_parse() {
        let src = r#"
            import Class'/Script/SteamCore.Friends';
            var Counter: int;
            local ExecuteUbergraph_X::Lobby: struct<SteamID, 8>;
            function Add(A: int, B: const ref struct<Vector, 12>, C: out array<object<Actor>>) -> int {
                local Tmp: float;
                ReturnValue = A;
            }
            event Clicked(Data: const ref struct<'/Script/X.Y', 16>, bOk: bool) { pop; }
            before Construct ubergraph { x = 1; }
            after Tick { return; }
            replace Foo { return; }
        "#;
        let items = parse(src).unwrap();
        assert_eq!(items.len(), 8);
        assert!(matches!(
            &items[0],
            Item::Import(Ref::Quoted { class: Some(_), .. })
        ));
        match &items[3] {
            Item::Patch(crate::patch::PatchItem::Function {
                params, ret, body, ..
            }) => {
                assert_eq!(params.len(), 3);
                assert_eq!(params[1].mode, crate::patch::Mode::ConstRef);
                assert_eq!(params[2].mode, crate::patch::Mode::Out);
                assert!(ret.is_some());
                assert_eq!(body.locals.len(), 1);
                assert_eq!(body.stmts.len(), 1);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            &items[5],
            Item::Patch(crate::patch::PatchItem::Hook { uber: true, .. })
        ));
    }
}
