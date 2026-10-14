//! C++ headers from a reflection database: every class, struct and enum at its real offsets,
//! plus parameter structs and ProcessEvent wrappers for every function.

use crate::{Db, Func, Index, Prop, Struct, Type};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

const KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "asm",
    "auto",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "delete",
    "do",
    "double",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "mutable",
    "namespace",
    "new",
    "not",
    "nullptr",
    "operator",
    "or",
    "private",
    "protected",
    "public",
    "register",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "template",
    "this",
    "throw",
    "true",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
    "xor",
    "Params",
    "fn",
    "p",
    "Path",
    "StaticClass",
];

pub fn ident(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o
}

struct Gen<'a> {
    idx: Index<'a>,
    names: HashMap<String, String>,
}

/// Generates `playkit_sdk.hpp` for the whole database.
pub fn generate(db: &Db) -> String {
    let mut g = Gen {
        idx: db.index(),
        names: HashMap::new(),
    };
    g.assign_names();
    g.header()
}

impl<'a> Gen<'a> {
    fn assign_names(&mut self) {
        let mut used = HashSet::new();
        let mut entries: Vec<(String, String)> = Vec::new();
        for c in &self.idx.db.classes {
            let base = ident(crate::short_name(&c.path));
            let name = if c.kind == "class" {
                let prefix = if self.idx.is_child_of(c, "/Script/Engine.Actor") {
                    "A"
                } else {
                    "U"
                };
                format!("{prefix}{base}")
            } else {
                format!("F{base}")
            };
            entries.push((c.path.clone(), name));
        }
        for e in &self.idx.db.enums {
            let base = ident(crate::short_name(&e.path));
            let name = if base.starts_with('E')
                && base.chars().nth(1).is_some_and(|c| c.is_ascii_uppercase())
            {
                base
            } else {
                format!("E{base}")
            };
            entries.push((e.path.clone(), name));
        }
        entries.sort();
        for (path, mut name) in entries {
            if !used.insert(name.clone()) {
                let pkg = ident(path.split('.').next().unwrap_or("").trim_start_matches('/'));
                name = format!("{name}_{pkg}");
                let mut n = 2;
                while !used.insert(name.clone()) {
                    name = format!("{name}{n}");
                    n += 1;
                }
            }
            self.names.insert(path.replace(':', "."), name);
        }
    }

    fn name_of(&self, path: &str) -> Option<String> {
        self.names.get(&path.replace(':', ".")).cloned()
    }

    fn ptr(&self, path: &str) -> String {
        self.name_of(path)
            .map_or("UObject*".into(), |n| format!("{n}*"))
    }

    /// C++ type of a value of type `t`, its size when known, and the struct it needs complete.
    fn type_cpp(&self, t: &Type, mask: u8, elem: i32) -> (String, Option<i32>, Option<&'a Struct>) {
        let simple = |s: &str, n| (s.to_string(), Some(n), None);
        match t {
            Type::Simple(k) => match k.as_str() {
                "Int8Property" => simple("int8_t", 1),
                "Int16Property" => simple("int16_t", 2),
                "IntProperty" => simple("int32_t", 4),
                "Int64Property" => simple("int64_t", 8),
                "UInt16Property" => simple("uint16_t", 2),
                "UInt32Property" => simple("uint32_t", 4),
                "UInt64Property" => simple("uint64_t", 8),
                "FloatProperty" => simple("float", 4),
                "DoubleProperty" => simple("double", 8),
                "BoolProperty" if mask == 0xFF => simple("bool", 1),
                "BoolProperty" | "ByteProperty" => simple("uint8_t", 1),
                "NameProperty" => simple("FName", 8),
                "StrProperty" => simple("FString", 16),
                "TextProperty" => simple("FText", 24),
                "MulticastDelegateProperty" => simple("FMulticastScriptDelegate", 16),
                _ => ("uint8_t".into(), None, None),
            },
            Type::Ref(k, path) => match k.as_str() {
                "ByteProperty" => match self.name_of(path) {
                    Some(n) => (n, Some(1), None),
                    None => simple("uint8_t", 1),
                },
                "EnumProperty" => match self.name_of(path) {
                    Some(n) if elem == 1 => (n, Some(1), None),
                    _ => ("uint8_t".into(), None, None),
                },
                "ObjectProperty" | "ObjectPtrProperty" => (self.ptr(path), Some(8), None),
                "ClassProperty" => simple("UClass*", 8),
                "WeakObjectProperty" => (
                    format!("TWeakObjectPtr<{}>", self.ptr(path).trim_end_matches('*')),
                    Some(8),
                    None,
                ),
                "SoftObjectProperty" => (
                    format!("TSoftObjectPtr<{}>", self.ptr(path).trim_end_matches('*')),
                    Some(40),
                    None,
                ),
                "SoftClassProperty" => simple("TSoftClassPtr", 40),
                "InterfaceProperty" => simple("FScriptInterface", 16),
                "DelegateProperty" => simple("FScriptDelegate", 16),
                "MulticastInlineDelegateProperty" | "MulticastDelegateProperty" => {
                    simple("FMulticastScriptDelegate", 16)
                }
                "Struct" => match (self.name_of(path), self.idx.get(path)) {
                    (Some(n), Some(s)) => (n, Some(s.size), Some(s)),
                    _ => ("uint8_t".into(), None, None),
                },
                _ => ("uint8_t".into(), None, None),
            },
            Type::Container(k, inner) if k == "ArrayProperty" => {
                let (i, _, _) = self.type_cpp(inner, 0xFF, 0);
                (format!("TArray<{i}>"), Some(16), None)
            }
            Type::Container(..) => simple("TSetOpaque", 80),
            Type::Map(..) => simple("TMapOpaque", 80),
        }
    }

    fn prop_cpp(&self, p: &Prop) -> (String, Option<i32>, Option<&'a Struct>) {
        let dim = p.dim.max(1);
        self.type_cpp(&Type::parse(&p.ty), p.mask, p.size / dim)
    }

    fn members(&self, props: &[Prop], start: i32, size: i32, out: &mut String) -> HashSet<String> {
        let mut props: Vec<&Prop> = props.iter().collect();
        props.sort_by_key(|p| p.offset);
        let mut at = start;
        let mut used = HashSet::new();
        for p in props {
            if p.offset < at || p.size <= 0 {
                continue;
            }
            if p.offset > at {
                let _ = writeln!(out, "    uint8_t pad_{at:x}[{}];", p.offset - at);
            }
            let mut name = ident(&p.name);
            while !used.insert(name.clone()) {
                name.push('_');
            }
            let dim = p.dim.max(1);
            let (ty, tsize, _) = self.prop_cpp(p);
            match tsize {
                Some(ts) if ts * dim == p.size => {
                    if dim > 1 {
                        let _ = writeln!(out, "    {ty} {name}[{dim}];");
                    } else {
                        let _ = writeln!(out, "    {ty} {name};");
                    }
                }
                _ => {
                    let _ = writeln!(out, "    uint8_t {name}[{}];", p.size);
                }
            }
            at = p.offset + p.size;
        }
        if size > at {
            let _ = writeln!(out, "    uint8_t pad_{at:x}[{}];", size - at);
        }
        used
    }

    fn function(
        &self,
        owner: &str,
        f: &Func,
        taken: &mut HashSet<String>,
        out: &mut String,
        params_out: &mut String,
    ) {
        let mut id = ident(&f.name);
        while !taken.insert(id.clone()) {
            id.push_str("_fn");
        }
        let pstruct = format!("{owner}_{id}_Params");
        let size = f.size.max(0);
        let _ = writeln!(params_out, "struct {pstruct} {{");
        self.members(&f.params, 0, size, params_out);
        if size == 0 {
            let _ = writeln!(params_out, "    uint8_t empty_;");
        }
        let _ = writeln!(params_out, "}};");
        let (mut args, mut sets, mut gets, mut ret) = (Vec::new(), Vec::new(), Vec::new(), None);
        let mut used = HashSet::new();
        for p in f.params() {
            let mut n = ident(&p.name);
            while !used.insert(n.clone()) {
                n.push('_');
            }
            let (ty, tsize, _) = self.prop_cpp(p);
            let typed = tsize == Some(p.size);
            if p.is_return() {
                if typed {
                    ret = Some((ty, n));
                }
                continue;
            }
            if !typed {
                continue;
            }
            if p.is_out() {
                args.push(format!("{ty}& {n}"));
                gets.push(n);
            } else {
                args.push(format!("{ty} const& {n}"));
                sets.push(n);
            }
        }
        let rt = ret.as_ref().map_or("void".to_string(), |(t, _)| t.clone());
        let _ = writeln!(
            out,
            "    {}{rt} {id}({}) {{",
            if f.is_static() { "static " } else { "" },
            args.join(", ")
        );
        let _ = writeln!(
            out,
            "        static void* fn = pk::function(StaticClass(), \"{}\");",
            escape(&f.name)
        );
        let _ = writeln!(out, "        {pstruct} p{{}};");
        for s in &sets {
            let _ = writeln!(out, "        p.{s} = {s};");
        }
        let target = if f.is_static() {
            "pk::default_object(StaticClass())"
        } else {
            "this"
        };
        let _ = writeln!(out, "        pk::process_event({target}, fn, &p);");
        for g in &gets {
            let _ = writeln!(out, "        {g} = p.{g};");
        }
        if let Some((_, n)) = ret {
            let _ = writeln!(out, "        return p.{n};");
        }
        let _ = writeln!(out, "    }}");
    }

    fn visit(&self, s: &'a Struct, done: &mut HashSet<String>, order: &mut Vec<&'a Struct>) {
        if self.name_of(&s.path).is_none() || !done.insert(s.path.clone()) {
            return;
        }
        if let Some(sup) = self.idx.get(&s.super_path) {
            self.visit(sup, done, order);
        }
        let mut deps = Vec::new();
        for p in s
            .properties
            .iter()
            .chain(s.functions.iter().flat_map(|f| f.params.iter()))
        {
            if let Type::Ref(k, path) = Type::parse(&p.ty) {
                if k == "Struct" {
                    deps.push(path);
                }
            }
        }
        for d in deps {
            if let Some(dep) = self.idx.get(&d) {
                self.visit(dep, done, order);
            }
        }
        order.push(s);
    }

    fn header(&self) -> String {
        let db = self.idx.db;
        let mut o = String::new();
        o.push_str("// Generated by Playkit from the reflection database of the game. Do not edit.\n#pragma once\n#include \"playkit.hpp\"\n\nnamespace sdk {\nusing namespace pk::types;\n#pragma pack(push, 1)\n\n");
        let mut enums: Vec<(String, &crate::Enum)> = db
            .enums
            .iter()
            .filter_map(|e| Some((self.name_of(&e.path)?, e)))
            .collect();
        enums.sort_by(|a, b| a.0.cmp(&b.0));
        for (n, e) in &enums {
            let _ = writeln!(o, "enum class {n} : uint8_t {{");
            let mut seen = HashSet::new();
            for (vn, v) in &e.values {
                let short = vn.rsplit("::").next().unwrap_or(vn);
                let mut id = ident(short);
                while !seen.insert(id.clone()) {
                    id.push('_');
                }
                if (0..=255).contains(v) {
                    let _ = writeln!(o, "    {id} = {v},");
                }
            }
            let _ = writeln!(o, "}};");
        }
        for c in &db.classes {
            if let Some(n) = self.name_of(&c.path) {
                let _ = writeln!(o, "struct {n};");
            }
        }
        o.push('\n');
        let mut done = HashSet::new();
        let mut order = Vec::new();
        for s in db
            .classes
            .iter()
            .filter(|c| c.kind != "class")
            .chain(db.classes.iter().filter(|c| c.kind == "class"))
        {
            self.visit(s, &mut done, &mut order);
        }
        for s in order {
            let Some(name) = self.name_of(&s.path) else {
                continue;
            };
            let sup = self.idx.get(&s.super_path);
            let sup_name = sup.and_then(|x| self.name_of(&x.path));
            let is_class = s.kind == "class";
            let size = s.size.max(0);
            let mut body = String::new();
            let start = match (&sup_name, is_class) {
                (Some(_), _) => sup.map_or(0, |x| x.size.max(0)),
                (None, true) => 0x28,
                (None, false) => 0,
            };
            let mut taken = self.members(&s.properties, start, size, &mut body);
            let (mut fns, mut params) = (String::new(), String::new());
            if is_class {
                for f in &s.functions {
                    self.function(&name, f, &mut taken, &mut fns, &mut params);
                }
            }
            o.push_str(&params);
            match (&sup_name, is_class) {
                (Some(sn), _) => {
                    let _ = writeln!(o, "struct {name} : {sn} {{");
                }
                (None, true) => {
                    let _ = writeln!(o, "struct {name} : UObjectBase {{");
                }
                (None, false) => {
                    let _ = writeln!(o, "struct {name} {{");
                }
            }
            let _ = writeln!(
                o,
                "    static constexpr const char* Path = \"{}\";",
                escape(&s.path)
            );
            let _ = writeln!(o, "    static UClass* StaticClass() {{ static UClass* c = pk::find<UClass>(Path); return c; }}");
            o.push_str(&body);
            o.push_str(&fns);
            let _ = writeln!(o, "}};");
            if size > 0 {
                let _ = writeln!(
                    o,
                    "static_assert(sizeof({name}) == {size}, \"{name} size\");\n"
                );
            }
        }
        o.push_str("\n#pragma pack(pop)\n} // namespace sdk\n");
        o
    }
}
