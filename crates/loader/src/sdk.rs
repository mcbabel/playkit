//! Writes the reflection database and C++ headers of everything currently loaded.
//! Request it by creating `playkit-sdk.request` next to the game executable; the output goes
//! to `playkit-sdk/` in the same folder.

use crate::ue::{self, Engine};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

pub fn check_request() {
    let Some(dir) = crate::loose::exe_dir() else {
        return;
    };
    let req = dir.join("playkit-sdk.request");
    if !req.exists() {
        return;
    }
    let _ = std::fs::remove_file(&req);
    let Some(e) = ue::get() else { return };
    let out = dir.join("playkit-sdk");
    let _ = std::fs::create_dir_all(&out);
    let model = Model::collect(e);
    let json = model.json(e);
    let hpp = model.cpp(e);
    let ok = std::fs::write(out.join("reflection.json"), json).is_ok()
        && std::fs::write(out.join("playkit_sdk.hpp"), hpp).is_ok();
    crate::log::write(&format!(
        "sdk: {} classes, {} structs, {} enums written to playkit-sdk ({})",
        model.classes.len(),
        model.structs.len(),
        model.enums.len(),
        if ok { "ok" } else { "write failed" }
    ));
}

unsafe fn rd<T: Copy>(a: usize) -> T {
    unsafe { std::ptr::read_unaligned(a as *const T) }
}

struct Model {
    classes: Vec<usize>,
    structs: Vec<usize>,
    enums: Vec<usize>,
    names: HashMap<usize, String>,
}

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

fn ident(s: &str) -> String {
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

impl Model {
    fn collect(e: &Engine) -> Model {
        let class_cls = e.find("/Script/CoreUObject.Class");
        let struct_cls = e.find("/Script/CoreUObject.ScriptStruct");
        let enum_cls = e.find("/Script/CoreUObject.Enum");
        let mut m = Model {
            classes: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            names: HashMap::new(),
        };
        let mut used = HashSet::new();
        for o in e.objects() {
            if e.object_name(o).starts_with("Default__") {
                continue;
            }
            let kind = if class_cls != 0 && e.is_a(o, class_cls) {
                m.classes.push(o);
                'C'
            } else if struct_cls != 0 && e.is_a(o, struct_cls) {
                m.structs.push(o);
                'S'
            } else if enum_cls != 0 && e.is_a(o, enum_cls) {
                m.enums.push(o);
                'E'
            } else {
                continue;
            };
            let base = ident(&e.object_name(o));
            let prefix = match kind {
                'C' => {
                    if m.is_actor(e, o) {
                        "A"
                    } else {
                        "U"
                    }
                }
                'S' => "F",
                _ => "",
            };
            let mut name = match kind {
                'E' if base.starts_with('E')
                    && base.chars().nth(1).is_some_and(|c| c.is_ascii_uppercase()) =>
                {
                    base.clone()
                }
                'E' => format!("E{base}"),
                _ => format!("{prefix}{base}"),
            };
            if !used.insert(name.clone()) {
                let pkg = ident(
                    e.path(o)
                        .split('.')
                        .next()
                        .unwrap_or("")
                        .trim_start_matches('/'),
                );
                name = format!("{name}_{pkg}");
                let mut n = 2;
                while !used.insert(name.clone()) {
                    name = format!("{name}{n}");
                    n += 1;
                }
            }
            m.names.insert(o, name);
        }
        m
    }

    fn is_actor(&self, e: &Engine, mut c: usize) -> bool {
        while c != 0 {
            if e.object_name(c) == "Actor" && e.path(c) == "/Script/Engine.Actor" {
                return true;
            }
            c = e.super_struct(c);
        }
        false
    }

    fn type_desc(&self, e: &Engine, p: usize) -> String {
        let t = e.property_type(p);
        let x = |o: usize| unsafe { rd::<usize>(p + ue::FPROPERTY_EXTRA + o) };
        let path = |o: usize| if o == 0 { String::new() } else { e.path(o) };
        match t.as_str() {
            "StructProperty" => format!("Struct<{}>", path(x(0))),
            "ObjectProperty" | "ObjectPtrProperty" | "WeakObjectProperty"
            | "LazyObjectProperty" | "SoftObjectProperty" | "InterfaceProperty" => {
                format!("{t}<{}>", path(x(0)))
            }
            "ClassProperty" | "SoftClassProperty" => format!("{t}<{}>", path(x(8))),
            "ArrayProperty" | "SetProperty" => format!("{t}<{}>", self.type_desc(e, x(0))),
            "MapProperty" => format!(
                "{t}<{},{}>",
                self.type_desc(e, x(0)),
                self.type_desc(e, x(8))
            ),
            "EnumProperty" => format!("{t}<{}>", path(x(8))),
            "ByteProperty" if x(0) != 0 => format!("ByteProperty<{}>", path(x(0))),
            "DelegateProperty"
            | "MulticastInlineDelegateProperty"
            | "MulticastSparseDelegateProperty" => {
                format!("{t}<{}>", path(x(0)))
            }
            _ => t,
        }
    }

    fn props_json(&self, e: &Engine, s: usize, out: &mut String) {
        out.push('[');
        for (i, p) in e.properties(s).into_iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"name\":\"{}\",\"type\":\"{}\",\"offset\":{},\"size\":{},\"flags\":{}}}",
                escape(&e.property_name(p)),
                escape(&self.type_desc(e, p)),
                e.property_offset(p),
                e.property_size(p),
                e.property_flags(p)
            );
        }
        out.push(']');
    }

    fn json(&self, e: &Engine) -> String {
        let mut o = String::from("{\"classes\":[");
        for (i, &c) in self.classes.iter().chain(self.structs.iter()).enumerate() {
            if i > 0 {
                o.push(',');
            }
            let sup = e.super_struct(c);
            let _ = write!(
                o,
                "\n{{\"kind\":\"{}\",\"path\":\"{}\",\"cpp\":\"{}\",\"super\":\"{}\",\"size\":{},\"properties\":",
                if i < self.classes.len() { "class" } else { "struct" },
                escape(&e.path(c)),
                escape(self.names.get(&c).map_or("", |s| s.as_str())),
                if sup == 0 { String::new() } else { escape(&e.path(sup)) },
                e.struct_size(c)
            );
            self.props_json(e, c, &mut o);
            o.push_str(",\"functions\":[");
            for (j, f) in functions(e, c).into_iter().enumerate() {
                if j > 0 {
                    o.push(',');
                }
                let _ = write!(
                    o,
                    "{{\"name\":\"{}\",\"flags\":{},\"size\":{},\"params\":",
                    escape(&e.object_name(f)),
                    e.function_flags(f),
                    e.struct_size(f)
                );
                self.props_json(e, f, &mut o);
                o.push('}');
            }
            o.push_str("]}");
        }
        o.push_str("],\n\"enums\":[");
        for (i, &en) in self.enums.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            let _ = write!(o, "\n{{\"path\":\"{}\",\"values\":[", escape(&e.path(en)));
            for (j, (n, v)) in enum_values(e, en).into_iter().enumerate() {
                if j > 0 {
                    o.push(',');
                }
                let _ = write!(o, "[\"{}\",{}]", escape(&n), v);
            }
            o.push_str("]}");
        }
        o.push_str("]}\n");
        o
    }

    /// C++ type for a property, its size in bytes when known, and the struct it needs complete.
    fn cpp_type(&self, e: &Engine, p: usize) -> (String, Option<i32>, Option<usize>) {
        let t = e.property_type(p);
        let x = |o: usize| unsafe { rd::<usize>(p + ue::FPROPERTY_EXTRA + o) };
        let named = |o: usize| self.names.get(&o).cloned();
        let ptr = |o: usize| named(o).map_or("UObject*".to_string(), |n| format!("{n}*"));
        match t.as_str() {
            "Int8Property" => ("int8_t".into(), Some(1), None),
            "Int16Property" => ("int16_t".into(), Some(2), None),
            "IntProperty" => ("int32_t".into(), Some(4), None),
            "Int64Property" => ("int64_t".into(), Some(8), None),
            "UInt16Property" => ("uint16_t".into(), Some(2), None),
            "UInt32Property" => ("uint32_t".into(), Some(4), None),
            "UInt64Property" => ("uint64_t".into(), Some(8), None),
            "FloatProperty" => ("float".into(), Some(4), None),
            "DoubleProperty" => ("double".into(), Some(8), None),
            "BoolProperty" => {
                let field_mask: u8 = unsafe { rd(p + ue::FPROPERTY_EXTRA + 3) };
                if field_mask == 0xFF {
                    ("bool".into(), Some(1), None)
                } else {
                    ("uint8_t".into(), Some(1), None)
                }
            }
            "ByteProperty" => match named(x(0)) {
                Some(n) => (n, Some(1), None),
                None => ("uint8_t".into(), Some(1), None),
            },
            "EnumProperty" => match (named(x(8)), unsafe {
                rd::<i32>(x(0) + ue::FPROPERTY_ELEMENT_SIZE)
            }) {
                (Some(n), 1) => (n, Some(1), None),
                _ => ("uint8_t".into(), None, None),
            },
            "NameProperty" => ("FName".into(), Some(8), None),
            "StrProperty" => ("FString".into(), Some(16), None),
            "TextProperty" => ("FText".into(), Some(24), None),
            "ObjectProperty" | "ObjectPtrProperty" => (ptr(x(0)), Some(8), None),
            "ClassProperty" => ("UClass*".into(), Some(8), None),
            "WeakObjectProperty" => (
                format!("TWeakObjectPtr<{}>", ptr(x(0)).trim_end_matches('*')),
                Some(8),
                None,
            ),
            "SoftObjectProperty" => (
                format!("TSoftObjectPtr<{}>", ptr(x(0)).trim_end_matches('*')),
                Some(40),
                None,
            ),
            "SoftClassProperty" => ("TSoftClassPtr".into(), Some(40), None),
            "InterfaceProperty" => ("FScriptInterface".into(), Some(16), None),
            "DelegateProperty" => ("FScriptDelegate".into(), Some(16), None),
            "MulticastInlineDelegateProperty" | "MulticastDelegateProperty" => {
                ("FMulticastScriptDelegate".into(), Some(16), None)
            }
            "StructProperty" => match named(x(0)) {
                Some(n) => (n, Some(e.struct_size(x(0))), Some(x(0))),
                None => ("uint8_t".into(), None, None),
            },
            "ArrayProperty" => {
                let (inner, _, _) = self.cpp_type(e, x(0));
                (format!("TArray<{inner}>"), Some(16), None)
            }
            "SetProperty" => ("TSetOpaque".into(), Some(80), None),
            "MapProperty" => ("TMapOpaque".into(), Some(80), None),
            _ => ("uint8_t".into(), None, None),
        }
    }

    fn members(
        &self,
        e: &Engine,
        s: usize,
        start: i32,
        size: i32,
        out: &mut String,
        deps: &mut Vec<usize>,
    ) -> HashSet<String> {
        let mut props = e.properties(s);
        props.sort_by_key(|&p| e.property_offset(p));
        let mut at = start;
        let mut used = HashSet::new();
        for p in props {
            let off = e.property_offset(p);
            let psize = e.property_size(p);
            if off < at || psize <= 0 {
                continue;
            }
            if off > at {
                let _ = writeln!(out, "    uint8_t pad_{at:x}[{}];", off - at);
            }
            let mut name = ident(&e.property_name(p));
            while !used.insert(name.clone()) {
                name.push('_');
            }
            let dim: i32 = unsafe { rd(p + ue::FPROPERTY_ARRAY_DIM) };
            let (ty, tsize, dep) = self.cpp_type(e, p);
            if let Some(d) = dep {
                deps.push(d);
            }
            match tsize {
                Some(ts) if ts * dim == psize => {
                    if dim > 1 {
                        let _ = writeln!(out, "    {ty} {name}[{dim}];");
                    } else {
                        let _ = writeln!(out, "    {ty} {name};");
                    }
                }
                _ => {
                    let _ = writeln!(out, "    uint8_t {name}[{psize}];");
                }
            }
            at = off + psize;
        }
        if size > at {
            let _ = writeln!(out, "    uint8_t pad_{at:x}[{}];", size - at);
        }
        used
    }

    fn function_cpp(
        &self,
        e: &Engine,
        owner: &str,
        f: usize,
        taken: &mut HashSet<String>,
        out: &mut String,
        params_out: &mut String,
    ) {
        let fname = e.object_name(f);
        let mut id = ident(&fname);
        while !taken.insert(id.clone()) {
            id.push_str("_fn");
        }
        let flags = e.function_flags(f);
        let pstruct = format!("{owner}_{id}_Params");
        let size = e.struct_size(f).max(0);
        let _ = writeln!(params_out, "struct {pstruct} {{");
        let mut deps = Vec::new();
        self.members(e, f, 0, size, params_out, &mut deps);
        if size == 0 {
            let _ = writeln!(params_out, "    uint8_t empty_;");
        }
        let _ = writeln!(params_out, "}};");
        let mut args = Vec::new();
        let mut sets = Vec::new();
        let mut gets = Vec::new();
        let mut ret = None;
        let mut used = HashSet::new();
        for p in e.properties(f) {
            let pf = e.property_flags(p);
            if pf & ue::CPF_PARM == 0 {
                continue;
            }
            let mut n = ident(&e.property_name(p));
            while !used.insert(n.clone()) {
                n.push('_');
            }
            let (ty, tsize, _) = self.cpp_type(e, p);
            let typed = tsize == Some(e.property_size(p));
            if pf & ue::CPF_RETURN_PARM != 0 {
                if typed {
                    ret = Some((ty, n));
                }
                continue;
            }
            if !typed {
                continue;
            }
            if pf & ue::CPF_OUT_PARM != 0 && pf & 0x2 == 0 {
                args.push(format!("{ty}& {n}"));
                gets.push(n);
            } else {
                args.push(format!("{ty} const& {n}"));
                sets.push(n);
            }
        }
        let is_static = flags & ue::FUNC_STATIC != 0;
        let rt = ret.as_ref().map_or("void".to_string(), |(t, _)| t.clone());
        let _ = writeln!(
            out,
            "    {}{rt} {id}({}) {{",
            if is_static { "static " } else { "" },
            args.join(", ")
        );
        let _ = writeln!(
            out,
            "        static void* fn = pk::function(StaticClass(), \"{}\");",
            escape(&fname)
        );
        let _ = writeln!(out, "        {pstruct} p{{}};");
        for s in &sets {
            let _ = writeln!(out, "        p.{s} = {s};");
        }
        let target = if is_static {
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

    fn cpp(&self, e: &Engine) -> String {
        let mut o = String::new();
        o.push_str("// Generated by the Playkit loader from the running game. Do not edit.\n#pragma once\n#include \"playkit.hpp\"\n\nnamespace sdk {\nusing namespace pk::types;\n#pragma pack(push, 1)\n\n");
        let mut enum_names: BTreeMap<String, usize> = BTreeMap::new();
        for &en in &self.enums {
            if let Some(n) = self.names.get(&en) {
                enum_names.insert(n.clone(), en);
            }
        }
        for (n, &en) in &enum_names {
            let _ = writeln!(o, "enum class {n} : uint8_t {{");
            let mut seen = HashSet::new();
            for (vn, v) in enum_values(e, en) {
                let short = vn.rsplit("::").next().unwrap_or(&vn);
                let mut id = ident(short);
                while !seen.insert(id.clone()) {
                    id.push('_');
                }
                if (0..=255).contains(&v) {
                    let _ = writeln!(o, "    {id} = {v},");
                }
            }
            let _ = writeln!(o, "}};");
        }
        for c in self.classes.iter().chain(self.structs.iter()) {
            if let Some(n) = self.names.get(c) {
                let _ = writeln!(o, "struct {n};");
            }
        }
        o.push('\n');
        let mut done: HashSet<usize> = HashSet::new();
        let mut order: Vec<usize> = Vec::new();
        for &s in self.structs.iter().chain(self.classes.iter()) {
            self.visit(e, s, &mut done, &mut order);
        }
        for s in order {
            let Some(name) = self.names.get(&s) else {
                continue;
            };
            let sup = e.super_struct(s);
            let sup_name = self.names.get(&sup);
            let start = if sup_name.is_some() {
                e.struct_size(sup).max(0)
            } else {
                0
            };
            let is_class = self.classes.contains(&s);
            let size = e.struct_size(s).max(0);
            let mut body = String::new();
            let mut deps = Vec::new();
            let mut taken = self.members(e, s, start, size, &mut body, &mut deps);
            let mut fns = String::new();
            let mut params = String::new();
            if is_class {
                for f in functions(e, s) {
                    self.function_cpp(e, name, f, &mut taken, &mut fns, &mut params);
                }
            }
            o.push_str(&params);
            match (sup_name, is_class) {
                (Some(sn), _) => {
                    let _ = writeln!(o, "struct {name} : {sn} {{");
                }
                (None, true) => {
                    let _ = writeln!(o, "struct {name} : UObjectBase {{");
                    body = String::new();
                    taken = self.members(e, s, 0x28, size, &mut body, &mut deps);
                }
                (None, false) => {
                    let _ = writeln!(o, "struct {name} {{");
                }
            }
            let _ = writeln!(
                o,
                "    static constexpr const char* Path = \"{}\";",
                escape(&e.path(s))
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

    fn visit(&self, e: &Engine, s: usize, done: &mut HashSet<usize>, order: &mut Vec<usize>) {
        if !self.names.contains_key(&s) || !done.insert(s) {
            return;
        }
        let sup = e.super_struct(s);
        if sup != 0 {
            self.visit(e, sup, done, order);
        }
        for p in e.properties(s) {
            if e.property_type(p) == "StructProperty" {
                let dep: usize = unsafe { rd(p + ue::FPROPERTY_EXTRA) };
                self.visit(e, dep, done, order);
            }
        }
        if self.classes.contains(&s) {
            for f in functions(e, s) {
                for p in e.properties(f) {
                    if e.property_type(p) == "StructProperty" {
                        let dep: usize = unsafe { rd(p + ue::FPROPERTY_EXTRA) };
                        self.visit(e, dep, done, order);
                    }
                }
            }
        }
        order.push(s);
    }
}

fn functions(_e: &Engine, class: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut f: usize = unsafe { rd(class + ue::USTRUCT_CHILDREN) };
    while f != 0 {
        out.push(f);
        f = unsafe { rd(f + ue::UFIELD_NEXT) };
    }
    out
}

fn enum_values(e: &Engine, en: usize) -> Vec<(String, i64)> {
    let data: usize = unsafe { rd(en + ue::UENUM_NAMES) };
    let num: i32 = unsafe { rd(en + ue::UENUM_NAMES + 8) };
    (0..num.max(0) as usize)
        .map(|i| {
            let at = data + 16 * i;
            (e.fname(at), unsafe { rd::<i64>(at + 8) })
        })
        .collect()
}
