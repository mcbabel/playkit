//! Reflection database of an Unreal Engine 4 game: classes, structs, enums, functions and
//! properties with their offsets, as written by the Playkit loader. Databases from several game
//! sessions can be merged, and C++ headers can be generated from them.

mod cpp;

pub use cpp::generate;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub const CPF_CONST_PARM: u64 = 0x2;
pub const CPF_PARM: u64 = 0x80;
pub const CPF_OUT_PARM: u64 = 0x100;
pub const CPF_RETURN_PARM: u64 = 0x400;
pub const FUNC_NATIVE: u32 = 0x400;
pub const FUNC_STATIC: u32 = 0x2000;
pub const FUNC_BLUEPRINT_PURE: u32 = 0x1000_0000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Db {
    #[serde(default)]
    pub classes: Vec<Struct>,
    #[serde(default)]
    pub enums: Vec<Enum>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Struct {
    /// "class" or "struct".
    pub kind: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cpp: String,
    #[serde(rename = "super", default)]
    pub super_path: String,
    pub size: i32,
    #[serde(default)]
    pub properties: Vec<Prop>,
    #[serde(default)]
    pub functions: Vec<Func>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prop {
    pub name: String,
    /// `IntProperty`, `Struct</Script/CoreUObject.Vector>`, `ArrayProperty<ObjectProperty</Script/Engine.Actor>>`, ...
    #[serde(rename = "type")]
    pub ty: String,
    pub offset: i32,
    /// Total size, element size times `dim`.
    pub size: i32,
    pub flags: u64,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub dim: i32,
    /// BoolProperty field mask; 0xFF is a native bool.
    #[serde(default = "full_mask", skip_serializing_if = "is_full_mask")]
    pub mask: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Func {
    pub name: String,
    pub flags: u32,
    pub size: i32,
    #[serde(default)]
    pub params: Vec<Prop>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Enum {
    pub path: String,
    #[serde(default)]
    pub values: Vec<(String, i64)>,
}

fn one() -> i32 {
    1
}
fn is_one(v: &i32) -> bool {
    *v == 1
}
fn full_mask() -> u8 {
    0xFF
}
fn is_full_mask(v: &u8) -> bool {
    *v == 0xFF
}

impl Prop {
    pub fn is_param(&self) -> bool {
        self.flags & CPF_PARM != 0
    }
    pub fn is_return(&self) -> bool {
        self.flags & CPF_RETURN_PARM != 0
    }
    /// An out parameter that is not `const&`, i.e. a real output.
    pub fn is_out(&self) -> bool {
        self.flags & CPF_OUT_PARM != 0 && self.flags & CPF_CONST_PARM == 0
    }
}

impl Func {
    pub fn is_static(&self) -> bool {
        self.flags & FUNC_STATIC != 0
    }
    pub fn is_native(&self) -> bool {
        self.flags & FUNC_NATIVE != 0
    }
    pub fn params(&self) -> impl Iterator<Item = &Prop> {
        self.params.iter().filter(|p| p.is_param())
    }
}

/// The parsed form of `Prop::ty`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Simple(String),
    /// Kind (`ObjectProperty`, `Struct`, ...) and the referenced object path.
    Ref(String, String),
    /// `ArrayProperty` or `SetProperty` with its element type.
    Container(String, Box<Type>),
    Map(Box<Type>, Box<Type>),
}

impl Type {
    pub fn parse(s: &str) -> Type {
        let s = s.trim();
        let Some(open) = s.find('<') else {
            return Type::Simple(s.to_string());
        };
        let kind = &s[..open];
        let inner = s[open + 1..].strip_suffix('>').unwrap_or(&s[open + 1..]);
        match kind {
            "ArrayProperty" | "SetProperty" => {
                Type::Container(kind.to_string(), Box::new(Type::parse(inner)))
            }
            "MapProperty" => {
                let mut depth = 0;
                let mut split = inner.len();
                for (i, c) in inner.char_indices() {
                    match c {
                        '<' => depth += 1,
                        '>' => depth -= 1,
                        ',' if depth == 0 => {
                            split = i;
                            break;
                        }
                        _ => {}
                    }
                }
                let (k, v) = inner.split_at(split);
                Type::Map(
                    Box::new(Type::parse(k)),
                    Box::new(Type::parse(v.trim_start_matches(','))),
                )
            }
            _ => Type::Ref(kind.to_string(), inner.to_string()),
        }
    }

    pub fn kind(&self) -> &str {
        match self {
            Type::Simple(k) | Type::Ref(k, _) | Type::Container(k, _) => k,
            Type::Map(..) => "MapProperty",
        }
    }

    /// The referenced object path (class, struct, enum or delegate signature), if any.
    pub fn target(&self) -> Option<&str> {
        match self {
            Type::Ref(_, p) if !p.is_empty() => Some(p),
            _ => None,
        }
    }
}

impl Db {
    pub fn load(path: &Path) -> Result<Db, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Adds everything from `newer`; entries with the same path are replaced by the newer ones.
    pub fn merge(&mut self, newer: Db) {
        let mut classes: BTreeMap<String, Struct> = self
            .classes
            .drain(..)
            .map(|c| (c.path.clone(), c))
            .collect();
        for c in newer.classes {
            classes.insert(c.path.clone(), c);
        }
        self.classes = classes.into_values().collect();
        let mut enums: BTreeMap<String, Enum> =
            self.enums.drain(..).map(|e| (e.path.clone(), e)).collect();
        for e in newer.enums {
            enums.insert(e.path.clone(), e);
        }
        self.enums = enums.into_values().collect();
    }

    pub fn index(&self) -> Index<'_> {
        Index::new(self)
    }
}

/// Fast lookups over a database.
pub struct Index<'a> {
    pub db: &'a Db,
    by_path: HashMap<String, &'a Struct>,
    enums: HashMap<String, &'a Enum>,
    by_name: HashMap<String, Vec<&'a Struct>>,
    functions: HashMap<String, Vec<(&'a Struct, &'a Func)>>,
}

/// `/Script/Engine.Actor` -> `Actor`, `/Game/X/Y.Y_C` -> `Y_C`.
pub fn short_name(path: &str) -> &str {
    path.rsplit(['.', ':']).next().unwrap_or(path)
}

fn key(path: &str) -> String {
    path.replace(':', ".")
}

impl<'a> Index<'a> {
    fn new(db: &'a Db) -> Self {
        let mut by_path = HashMap::new();
        let mut by_name: HashMap<String, Vec<&Struct>> = HashMap::new();
        let mut functions: HashMap<String, Vec<(&Struct, &Func)>> = HashMap::new();
        for c in &db.classes {
            by_path.insert(key(&c.path), c);
            by_name
                .entry(short_name(&c.path).to_string())
                .or_default()
                .push(c);
            for f in &c.functions {
                functions.entry(f.name.clone()).or_default().push((c, f));
            }
        }
        let enums = db.enums.iter().map(|e| (key(&e.path), e)).collect();
        Index {
            db,
            by_path,
            enums,
            by_name,
            functions,
        }
    }

    pub fn get(&self, path: &str) -> Option<&'a Struct> {
        self.by_path.get(&key(path)).copied()
    }

    pub fn get_enum(&self, path: &str) -> Option<&'a Enum> {
        self.enums.get(&key(path)).copied()
    }

    /// Classes and structs with this short name, e.g. `KismetSystemLibrary` or `Vector`.
    pub fn named(&self, name: &str) -> &[&'a Struct] {
        self.by_name.get(name).map_or(&[], |v| v.as_slice())
    }

    /// Every class that declares a function with this name.
    pub fn functions_named(&self, name: &str) -> &[(&'a Struct, &'a Func)] {
        self.functions.get(name).map_or(&[], |v| v.as_slice())
    }

    /// Super chain, starting with `s` itself.
    pub fn chain(&self, s: &'a Struct) -> Vec<&'a Struct> {
        let mut out = vec![s];
        let mut cur = s;
        while let Some(sup) = self.get(&cur.super_path) {
            if out.iter().any(|x| std::ptr::eq(*x, sup)) {
                break;
            }
            out.push(sup);
            cur = sup;
        }
        out
    }

    /// Function of a class or one of its parents.
    pub fn function(&self, class: &'a Struct, name: &str) -> Option<(&'a Struct, &'a Func)> {
        self.chain(class)
            .into_iter()
            .find_map(|c| c.functions.iter().find(|f| f.name == name).map(|f| (c, f)))
    }

    /// Property of a class or struct or one of its parents.
    pub fn property(&self, s: &'a Struct, name: &str) -> Option<(&'a Struct, &'a Prop)> {
        self.chain(s)
            .into_iter()
            .find_map(|c| c.properties.iter().find(|p| p.name == name).map(|p| (c, p)))
    }

    pub fn is_child_of(&self, s: &'a Struct, ancestor: &str) -> bool {
        let k = key(ancestor);
        self.chain(s).iter().any(|c| key(&c.path) == k)
    }

    /// Enums with this short name, e.g. `ESlateVisibility`.
    pub fn enums_named(&self, name: &str) -> Vec<&'a Enum> {
        self.db
            .enums
            .iter()
            .filter(|e| short_name(&e.path) == name)
            .collect()
    }

    /// Value of an enumerator, given as `Name` or `Enum::Name`.
    /// Unreal names ignore case, so a different spelling matches when no exact one does.
    pub fn enum_value(&self, e: &Enum, name: &str) -> Option<i64> {
        let bare = name.rsplit("::").next().unwrap_or(name);
        let short = |n: &str| n.rsplit("::").next().unwrap_or(n).to_string();
        e.values
            .iter()
            .find(|(n, _)| n == name || short(n) == bare)
            .or_else(|| {
                e.values
                    .iter()
                    .find(|(n, _)| short(n).eq_ignore_ascii_case(bare))
            })
            .map(|(_, v)| *v)
    }

    /// Like [`Index::function`], but ignores case when no exact name matches, like Unreal's
    /// names do.
    pub fn function_nocase(&self, class: &'a Struct, name: &str) -> Option<(&'a Struct, &'a Func)> {
        self.function(class, name).or_else(|| {
            self.chain(class).into_iter().find_map(|c| {
                c.functions
                    .iter()
                    .find(|f| f.name.eq_ignore_ascii_case(name))
                    .map(|f| (c, f))
            })
        })
    }

    /// Like [`Index::property`], but ignores case when no exact name matches.
    pub fn property_nocase(&self, s: &'a Struct, name: &str) -> Option<(&'a Struct, &'a Prop)> {
        self.property(s, name).or_else(|| {
            self.chain(s).into_iter().find_map(|c| {
                c.properties
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(name))
                    .map(|p| (c, p))
            })
        })
    }

    /// Like [`Index::functions_named`], but ignores case when no exact name matches.
    pub fn functions_named_nocase(&self, name: &str) -> Vec<(&'a Struct, &'a Func)> {
        let exact = self.functions_named(name);
        if !exact.is_empty() {
            return exact.to_vec();
        }
        self.functions
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .flat_map(|(_, v)| v.iter().copied())
            .collect()
    }

    /// Object paths referenced by property or parameter types whose short name is `name`.
    /// Finds objects that are not classes or structs themselves, such as delegate signatures
    /// declared at package level.
    pub fn type_targets_named(&self, name: &str) -> Vec<&'a str> {
        let mut out: Vec<&'a str> = vec![];
        let props = self.db.classes.iter().flat_map(|c| {
            c.properties
                .iter()
                .chain(c.functions.iter().flat_map(|f| f.params.iter()))
        });
        for p in props {
            if !p.ty.contains(name) {
                continue;
            }
            for part in p.ty.split(['<', '>', ',']) {
                if part.starts_with('/') && short_name(part) == name && !out.contains(&part) {
                    out.push(part);
                }
            }
        }
        out
    }

    /// Up to `n` short names of classes, structs and enums that look like `name`.
    pub fn similar_types(&self, name: &str, n: usize) -> Vec<&'a str> {
        let enums = self.db.enums.iter().map(|e| short_name(&e.path));
        let structs = self.db.classes.iter().map(|c| short_name(&c.path));
        closest(name, structs.chain(enums), n)
    }

    /// Up to `n` function names of the whole database that look like `name`.
    pub fn similar_functions(&self, name: &str, n: usize) -> Vec<&'a str> {
        let names = self
            .db
            .classes
            .iter()
            .flat_map(|c| c.functions.iter().map(|f| f.name.as_str()));
        closest(name, names, n)
    }

    /// Up to `n` properties and functions of `s` and its parents that look like `name`.
    pub fn similar_members(&self, s: &'a Struct, name: &str, n: usize) -> Vec<&'a str> {
        let chain = self.chain(s);
        let names = chain.iter().flat_map(|c| {
            c.properties
                .iter()
                .map(|p| p.name.as_str())
                .chain(c.functions.iter().map(|f| f.name.as_str()))
        });
        closest(name, names, n)
    }
}

fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// Names close to `name`: small edit distance, or containing it (ignoring case).
fn closest<'b>(name: &str, cands: impl Iterator<Item = &'b str>, n: usize) -> Vec<&'b str> {
    let lower = name.to_lowercase();
    let limit = (name.chars().count() / 3).max(2);
    let mut scored: Vec<(usize, &'b str)> = cands
        .filter_map(|c| {
            let d = distance(name, c);
            if d <= limit {
                Some((d, c))
            } else if lower.len() >= 4 && c.to_lowercase().contains(&lower) {
                Some((limit + c.len() - lower.len(), c))
            } else {
                None
            }
        })
        .collect();
    scored.sort();
    scored.dedup_by(|a, b| a.1 == b.1);
    scored.into_iter().take(n).map(|(_, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_types() {
        assert_eq!(
            Type::parse("ArrayProperty<ObjectProperty</Script/Engine.Actor>>"),
            Type::Container(
                "ArrayProperty".into(),
                Box::new(Type::Ref(
                    "ObjectProperty".into(),
                    "/Script/Engine.Actor".into()
                ))
            )
        );
        let m = Type::parse("MapProperty<NameProperty,Struct</Script/CoreUObject.Vector>>");
        assert_eq!(
            m,
            Type::Map(
                Box::new(Type::Simple("NameProperty".into())),
                Box::new(Type::Ref(
                    "Struct".into(),
                    "/Script/CoreUObject.Vector".into()
                ))
            )
        );
        assert_eq!(Type::parse("IntProperty").kind(), "IntProperty");
    }

    #[test]
    fn merge_keeps_old_and_takes_newer() {
        let s = |path: &str, size| Struct {
            kind: "class".into(),
            path: path.into(),
            cpp: String::new(),
            super_path: String::new(),
            size,
            properties: vec![],
            functions: vec![],
        };
        let mut a = Db {
            classes: vec![s("/Script/A.X", 1), s("/Script/A.Y", 2)],
            enums: vec![],
        };
        a.merge(Db {
            classes: vec![s("/Script/A.Y", 3), s("/Game/B.Z_C", 4)],
            enums: vec![],
        });
        let i = a.index();
        assert_eq!(a.classes.len(), 3);
        assert_eq!(i.get("/Script/A.Y").unwrap().size, 3);
        assert_eq!(i.named("Z_C").len(), 1);
    }

    #[test]
    fn enums_signatures_and_suggestions() {
        let db: Db = serde_json::from_str(
            r#"{"classes": [
                {"kind": "class", "path": "/Script/UMG.Widget", "size": 8, "properties": [
                    {"name": "Visibility", "type": "EnumProperty</Script/UMG.ESlateVisibility>", "offset": 0, "size": 1, "flags": 0},
                    {"name": "OnClicked", "type": "MulticastInlineDelegateProperty</Script/UMG.OnClicked__DelegateSignature>", "offset": 0, "size": 16, "flags": 0}
                ], "functions": [{"name": "SetVisibility", "flags": 0, "size": 1, "params": []}]}
            ], "enums": [
                {"path": "/Script/UMG.ESlateVisibility", "values": [["ESlateVisibility::Visible", 0], ["ESlateVisibility::Collapsed", 1]]},
                {"path": "/Script/SlateCore.EHorizontalAlignment", "values": [["HAlign_Fill", 0], ["HAlign_Left", 1]]}
            ]}"#,
        )
        .unwrap();
        let i = db.index();
        let e = i.enums_named("ESlateVisibility")[0];
        assert_eq!(i.enum_value(e, "Collapsed"), Some(1));
        assert_eq!(i.enum_value(e, "ESlateVisibility::Collapsed"), Some(1));
        assert_eq!(i.enum_value(e, "Gone"), None);
        assert_eq!(i.enum_value(e, "collapsed"), Some(1));
        let w = i.get("/Script/UMG.Widget").unwrap();
        assert_eq!(
            i.property_nocase(w, "visibility").unwrap().1.name,
            "Visibility"
        );
        assert_eq!(
            i.function_nocase(w, "setvisibility").unwrap().1.name,
            "SetVisibility"
        );
        assert_eq!(i.functions_named_nocase("SETVISIBILITY").len(), 1);
        let h = i.enums_named("EHorizontalAlignment")[0];
        assert_eq!(
            i.enum_value(h, "EHorizontalAlignment::HAlign_Left"),
            Some(1)
        );
        assert_eq!(
            i.type_targets_named("OnClicked__DelegateSignature"),
            ["/Script/UMG.OnClicked__DelegateSignature"]
        );
        assert_eq!(i.similar_types("Widgte", 3), ["Widget"]);
        assert_eq!(i.similar_functions("SetVisiblity", 3), ["SetVisibility"]);
        let w = i.get("/Script/UMG.Widget").unwrap();
        assert_eq!(i.similar_members(w, "visibility", 3)[0], "Visibility");
    }
}
