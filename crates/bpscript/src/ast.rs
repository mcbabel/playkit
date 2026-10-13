use std::fmt::{self, Display, Formatter, Write};

#[derive(Clone, Debug, PartialEq)]
pub struct Name {
    pub text: String,
    pub number: Option<i32>,
}

impl Name {
    pub fn new(text: impl Into<String>) -> Self {
        Name {
            text: text.into(),
            number: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ref {
    Path(Vec<Name>),
    Quoted { class: Option<Name>, path: String },
    Index(i32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ptr {
    pub owner: Option<Ref>,
    pub path: Vec<Name>,
}

impl Ptr {
    pub fn bare(name: Name) -> Self {
        Ptr {
            owner: None,
            path: vec![name],
        }
    }

    pub fn null() -> Self {
        Ptr {
            owner: Some(Ref::Index(0)),
            path: vec![],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    Local,
    Out,
    Inst,
    Default,
    Sparse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallKind {
    Math,
    Final,
    LocalFinal,
    Virtual,
    LocalVirtual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtxOp {
    Normal,
    FailSilent,
    Class,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastKind {
    Dynamic,
    Meta,
    ObjToIface,
    CrossIface,
    IfaceToObj,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LetKw {
    Let,
    Bool,
    Obj,
    Weak,
    Delegate,
    Multicast,
    Frame,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Label(String),
    Raw(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Lit {
    Int(i32),
    Int64(i64),
    UInt64(u64),
    Byte(u8),
    IntByte(u8),
    IntZero,
    IntOne,
    Float(u32),
    True,
    False,
    SelfObj,
    NoObject,
    NoInterface,
    Nothing,
    Str(String),
    AnsiStr(String),
    UniStr(String),
    Name(Name),
    Vector([u32; 3]),
    Rotator([u32; 3]),
    Transform([u32; 10]),
    Token(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Text {
    Empty,
    Localized(Expr, Expr, Expr),
    Invariant(Expr),
    Literal(Expr),
    Table(Ref, Expr, Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Var(Ptr),
    KindVar(VarKind, Ptr),
    Obj(Ref),
    Lit(Lit),
    Call {
        kind: Option<CallKind>,
        head: Ref,
        args: Vec<Expr>,
    },
    Ctx {
        obj: Box<Expr>,
        op: CtxOp,
        rv: Option<Ptr>,
        skip: Option<u32>,
        inner: Box<Expr>,
    },
    Member(Box<Expr>, Ptr),
    Index(Box<Expr>, Box<Expr>),
    Cast(CastKind, Ref, Box<Expr>),
    PrimCast(String, Box<Expr>),
    Struct {
        ty: Ref,
        size: Option<i32>,
        fields: Vec<Expr>,
    },
    Switch {
        index: Box<Expr>,
        cases: Vec<(Expr, Expr)>,
        default: Box<Expr>,
    },
    Addr(Target),
    SkipOffset(Target),
    Text(Box<Text>),
    Bind(Box<Expr>, Box<Expr>, Name),
    AddMulti(Box<Expr>, Box<Expr>),
    RemoveMulti(Box<Expr>, Box<Expr>),
    ClearMulti(Box<Expr>),
    Broadcast {
        sig: Option<Ref>,
        del: Box<Expr>,
        args: Vec<Expr>,
    },
    Assert(Box<Expr>, u16, bool),
    Instrument(String, Option<Name>),
    FieldPath(Box<Expr>),
    Property(Ptr),
    SoftObj(Box<Expr>),
    Delegate(Name),
    Interface(Box<Expr>),
    Skip(Box<Expr>),
    ArrayConst(Ptr, Vec<Expr>),
    SetConst(Ptr, Vec<Expr>),
    MapConst(Ptr, Ptr, Vec<(Expr, Expr)>),
    Stmt(Box<Stmt>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Label(String),
    Expr(Expr),
    Let {
        kw: Option<LetKw>,
        ptr: Option<Ptr>,
        dest: Box<Expr>,
        value: Box<Expr>,
    },
    Goto(Target),
    GotoComputed(Expr),
    IfNot(Expr, Target),
    Push(Target),
    Pop,
    PopIfNot(Expr),
    Return(Option<Expr>),
    Nop,
    End,
    SetArray(Expr, Vec<Expr>),
    SetSet(Expr, Vec<Expr>),
    SetMap(Expr, Vec<(Expr, Expr)>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: Ref,
    pub noend: bool,
    pub body: Vec<Stmt>,
}

pub const KEYWORDS: &[&str] = &[
    "function",
    "import",
    "goto",
    "if",
    "push",
    "pop",
    "return",
    "nop",
    "end",
    "let",
    "letbool",
    "letobj",
    "letweak",
    "letdelegate",
    "letmulticast",
    "letframe",
    "local",
    "out",
    "inst",
    "default",
    "sparse",
    "math",
    "final",
    "localfinal",
    "virtual",
    "localvirtual",
    "self",
    "none",
    "nointerface",
    "nothing",
    "true",
    "false",
    "intzero",
    "intone",
    "intbyte",
    "float",
    "vec",
    "rot",
    "transform",
    "obj",
    "cast",
    "metacast",
    "objtoiface",
    "crossiface",
    "ifacetoobj",
    "primcast",
    "switch",
    "skipoffset",
    "emptytext",
    "nsloctext",
    "invtext",
    "text",
    "loctable",
    "bind",
    "addmulti",
    "removemulti",
    "clearmulti",
    "clear",
    "broadcast",
    "assert",
    "instrument",
    "fieldpath",
    "property",
    "softobj",
    "delegate",
    "interface",
    "skip",
    "arrayconst",
    "setconst",
    "mapconst",
    "token",
    "set",
    "map",
    "class",
    "struct",
    "var",
    "before",
    "after",
    "replace",
    "event",
    "noend",
    "ref",
    "const",
    "array",
];

pub fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(x) if x.is_ascii_alphabetic() || x == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
        && !KEYWORDS.contains(&s)
}

pub fn quote_str(s: &str, out: &mut String) {
    out.push('"');
    escape(s, '"', out);
    out.push('"');
}

fn escape(s: &str, q: char, out: &mut String) {
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
}

impl Display for Name {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if is_ident(&self.text) {
            f.write_str(&self.text)?;
        } else {
            let mut s = String::from("`");
            escape(&self.text, '`', &mut s);
            s.push('`');
            f.write_str(&s)?;
        }
        if let Some(n) = self.number {
            write!(f, "#{n}")?;
        }
        Ok(())
    }
}

impl Display for Ref {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Ref::Path(segs) => join(f, segs, "::"),
            Ref::Quoted { class, path } => {
                if let Some(c) = class {
                    write!(f, "{c}")?;
                }
                let mut s = String::from("'");
                escape(path, '\'', &mut s);
                s.push('\'');
                f.write_str(&s)
            }
            Ref::Index(i) => write!(f, "#{i}"),
        }
    }
}

impl Display for Ptr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() && self.owner == Some(Ref::Index(0)) {
            return f.write_str("none");
        }
        if let Some(o) = &self.owner {
            write!(f, "{o}::")?;
        }
        if self.path.len() == 1 {
            write!(f, "{}", self.path[0])
        } else {
            f.write_str("{")?;
            join(f, &self.path, ", ")?;
            f.write_str("}")
        }
    }
}

fn join<T: Display>(f: &mut Formatter<'_>, items: &[T], sep: &str) -> fmt::Result {
    for (i, x) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(sep)?;
        }
        write!(f, "{x}")?;
    }
    Ok(())
}

impl Display for Target {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Target::Label(l) => f.write_str(l),
            Target::Raw(n) => write!(f, "{n}"),
        }
    }
}

pub fn float_text(bits: u32) -> String {
    let v = f32::from_bits(bits);
    if !v.is_finite() {
        return format!("float(0x{bits:08x})");
    }
    let s = format!("{v:?}");
    if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{s}.0")
    }
}

fn floats(f: &mut Formatter<'_>, kw: &str, v: &[u32]) -> fmt::Result {
    write!(f, "{kw}(")?;
    for (i, b) in v.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        f.write_str(&float_text(*b))?;
    }
    f.write_str(")")
}

impl Display for Lit {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut s = String::new();
        let t: &str = match self {
            Lit::Int(v) => return write!(f, "{v}"),
            Lit::Int64(v) => return write!(f, "{v}i64"),
            Lit::UInt64(v) => return write!(f, "{v}u64"),
            Lit::Byte(v) => return write!(f, "{v}u8"),
            Lit::IntByte(v) => return write!(f, "intbyte({v})"),
            Lit::IntZero => "intzero",
            Lit::IntOne => "intone",
            Lit::Float(b) => return f.write_str(&float_text(*b)),
            Lit::True => "true",
            Lit::False => "false",
            Lit::SelfObj => "self",
            Lit::NoObject => "none",
            Lit::NoInterface => "nointerface",
            Lit::Nothing => "nothing",
            Lit::Str(v) => {
                quote_str(v, &mut s);
                &s
            }
            Lit::AnsiStr(v) => {
                s.push('a');
                quote_str(v, &mut s);
                &s
            }
            Lit::UniStr(v) => {
                s.push('u');
                quote_str(v, &mut s);
                &s
            }
            Lit::Name(n) => {
                s.push('n');
                quote_str(&n.text, &mut s);
                if let Some(k) = n.number {
                    let _ = write!(s, "#{k}");
                }
                &s
            }
            Lit::Vector(v) => return floats(f, "vec", v),
            Lit::Rotator(v) => return floats(f, "rot", v),
            Lit::Transform(v) => return floats(f, "transform", v),
            Lit::Token(t) => return write!(f, "token({t})"),
        };
        f.write_str(t)
    }
}

fn args(f: &mut Formatter<'_>, items: &[Expr]) -> fmt::Result {
    f.write_str("(")?;
    join(f, items, ", ")?;
    f.write_str(")")
}

fn pairs(f: &mut Formatter<'_>, items: &[(Expr, Expr)], sep: &str) -> fmt::Result {
    for (i, (k, v)) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{k}{sep}{v}")?;
    }
    Ok(())
}

impl Display for VarKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            VarKind::Local => "local",
            VarKind::Out => "out",
            VarKind::Inst => "inst",
            VarKind::Default => "default",
            VarKind::Sparse => "sparse",
        })
    }
}

impl Display for CallKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CallKind::Math => "math",
            CallKind::Final => "final",
            CallKind::LocalFinal => "localfinal",
            CallKind::Virtual => "virtual",
            CallKind::LocalVirtual => "localvirtual",
        })
    }
}

impl CastKind {
    pub fn keyword(self) -> &'static str {
        match self {
            CastKind::Dynamic => "cast",
            CastKind::Meta => "metacast",
            CastKind::ObjToIface => "objtoiface",
            CastKind::CrossIface => "crossiface",
            CastKind::IfaceToObj => "ifacetoobj",
        }
    }
}

impl LetKw {
    pub fn keyword(self) -> &'static str {
        match self {
            LetKw::Let => "let",
            LetKw::Bool => "letbool",
            LetKw::Obj => "letobj",
            LetKw::Weak => "letweak",
            LetKw::Delegate => "letdelegate",
            LetKw::Multicast => "letmulticast",
            LetKw::Frame => "letframe",
        }
    }
}

impl Display for Text {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Text::Empty => f.write_str("emptytext"),
            Text::Localized(ns, key, src) => write!(f, "nsloctext({ns}, {key}, {src})"),
            Text::Invariant(e) => write!(f, "invtext({e})"),
            Text::Literal(e) => write!(f, "text({e})"),
            Text::Table(r, id, key) => write!(f, "loctable({r}, {id}, {key})"),
        }
    }
}

impl Display for Expr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Var(p) => write!(f, "{p}"),
            Expr::KindVar(k, p) => write!(f, "{k} {p}"),
            Expr::Obj(r) => write!(f, "obj({r})"),
            Expr::Lit(l) => write!(f, "{l}"),
            Expr::Call {
                kind,
                head,
                args: a,
            } => {
                if let Some(k) = kind {
                    write!(f, "{k} ")?;
                }
                write!(f, "{head}")?;
                args(f, a)
            }
            Expr::Ctx {
                obj,
                op,
                rv,
                skip,
                inner,
            } => {
                write!(f, "{obj}")?;
                f.write_str(match op {
                    CtxOp::Normal => ".",
                    CtxOp::FailSilent => "?.",
                    CtxOp::Class => "@.",
                })?;
                if rv.is_some() || skip.is_some() {
                    f.write_str("[")?;
                    if let Some(p) = rv {
                        write!(f, "rv={p}")?;
                    }
                    if let Some(s) = skip {
                        if rv.is_some() {
                            f.write_str(", ")?;
                        }
                        write!(f, "skip={s}")?;
                    }
                    f.write_str("]")?;
                }
                write!(f, "{inner}")
            }
            Expr::Member(e, p) => write!(f, "{e}->{p}"),
            Expr::Index(a, i) => write!(f, "{a}[{i}]"),
            Expr::Cast(k, c, e) => write!(f, "{}<{c}>({e})", k.keyword()),
            Expr::PrimCast(t, e) => write!(f, "primcast<{t}>({e})"),
            Expr::Struct { ty, size, fields } => {
                write!(f, "{ty}")?;
                if let Some(s) = size {
                    write!(f, "<{s}>")?;
                }
                f.write_str("{")?;
                join(f, fields, ", ")?;
                f.write_str("}")
            }
            Expr::Switch {
                index,
                cases,
                default,
            } => {
                write!(f, "switch({index}) {{")?;
                pairs(f, cases, " => ")?;
                if !cases.is_empty() {
                    f.write_str(", ")?;
                }
                write!(f, "_ => {default}}}")
            }
            Expr::Addr(t) => write!(f, "@{t}"),
            Expr::SkipOffset(t) => write!(f, "skipoffset({t})"),
            Expr::Text(t) => write!(f, "{t}"),
            Expr::Bind(d, o, n) => write!(f, "bind({d}, {o}, {n})"),
            Expr::AddMulti(a, b) => write!(f, "addmulti({a}, {b})"),
            Expr::RemoveMulti(a, b) => write!(f, "removemulti({a}, {b})"),
            Expr::ClearMulti(a) => write!(f, "clearmulti({a})"),
            Expr::Broadcast { sig, del, args: a } => {
                f.write_str("broadcast")?;
                if let Some(s) = sig {
                    write!(f, "<{s}>")?;
                }
                write!(f, "({del}")?;
                for x in a {
                    write!(f, ", {x}")?;
                }
                f.write_str(")")
            }
            Expr::Assert(e, line, dbg) => write!(f, "assert({e}, {line}, {dbg})"),
            Expr::Instrument(t, n) => match n {
                Some(n) => write!(f, "instrument({t}, {n})"),
                None => write!(f, "instrument({t})"),
            },
            Expr::FieldPath(e) => write!(f, "fieldpath({e})"),
            Expr::Property(p) => write!(f, "property({p})"),
            Expr::SoftObj(e) => write!(f, "softobj({e})"),
            Expr::Delegate(n) => write!(f, "delegate({n})"),
            Expr::Interface(e) => write!(f, "interface({e})"),
            Expr::Skip(e) => write!(f, "skip({e})"),
            Expr::ArrayConst(p, v) => {
                write!(f, "arrayconst<{p}>[")?;
                join(f, v, ", ")?;
                f.write_str("]")
            }
            Expr::SetConst(p, v) => {
                write!(f, "setconst<{p}>[")?;
                join(f, v, ", ")?;
                f.write_str("]")
            }
            Expr::MapConst(k, v, items) => {
                write!(f, "mapconst<{k}, {v}>[")?;
                pairs(f, items, ": ")?;
                f.write_str("]")
            }
            Expr::Stmt(s) => {
                f.write_str("{ ")?;
                s.fmt_body(f)?;
                f.write_str(" }")
            }
        }
    }
}

impl Stmt {
    fn fmt_body(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Stmt::Label(l) => write!(f, "{l}:"),
            Stmt::Expr(e) => write!(f, "{e}"),
            Stmt::Let {
                kw,
                ptr,
                dest,
                value,
            } => {
                if let Some(k) = kw {
                    f.write_str(k.keyword())?;
                    if let Some(p) = ptr {
                        write!(f, "({p})")?;
                    }
                    f.write_str(" ")?;
                }
                write!(f, "{dest} = {value}")
            }
            Stmt::Goto(t) => write!(f, "goto {t}"),
            Stmt::GotoComputed(e) => write!(f, "goto *{e}"),
            Stmt::IfNot(e, t) => write!(f, "if !{e} goto {t}"),
            Stmt::Push(t) => write!(f, "push {t}"),
            Stmt::Pop => f.write_str("pop"),
            Stmt::PopIfNot(e) => write!(f, "pop if !{e}"),
            Stmt::Return(None) => f.write_str("return"),
            Stmt::Return(Some(e)) => write!(f, "return {e}"),
            Stmt::Nop => f.write_str("nop"),
            Stmt::End => f.write_str("end"),
            Stmt::SetArray(d, v) => {
                write!(f, "{d} = [")?;
                join(f, v, ", ")?;
                f.write_str("]")
            }
            Stmt::SetSet(d, v) => {
                write!(f, "{d} = set[")?;
                join(f, v, ", ")?;
                f.write_str("]")
            }
            Stmt::SetMap(d, v) => {
                write!(f, "{d} = map[")?;
                pairs(f, v, ": ")?;
                f.write_str("]")
            }
        }
    }
}

impl Display for Stmt {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Stmt::Label(l) => write!(f, "{l}:"),
            Stmt::Expr(Expr::AddMulti(a, b)) => write!(f, "{a} += {b};"),
            Stmt::Expr(Expr::RemoveMulti(a, b)) => write!(f, "{a} -= {b};"),
            Stmt::Expr(Expr::ClearMulti(a)) => write!(f, "clear {a};"),
            s => {
                s.fmt_body(f)?;
                f.write_str(";")
            }
        }
    }
}

impl Display for Function {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "function {}", self.name)?;
        if self.noend {
            f.write_str(" noend")?;
        }
        f.write_str(" {\n")?;
        for s in &self.body {
            match s {
                Stmt::Label(_) => writeln!(f, "{s}")?,
                _ => writeln!(f, "    {s}")?,
            }
        }
        f.write_str("}\n")
    }
}
