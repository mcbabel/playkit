use crate::ast::{Name, Ref};
use crate::Asset;
use std::collections::HashMap;
use unreal_asset::exports::ExportBaseTrait;
use unreal_asset::types::fname::FName;
use unreal_asset::types::PackageIndex;
use unreal_asset::Import;

pub fn fname_text(n: &FName) -> String {
    let c = n.get_content();
    match n.get_number() {
        0 => c,
        k => format!("{c}_{}", k - 1),
    }
}

pub fn split_number(text: &str) -> Option<(&str, i32)> {
    let i = text.rfind('_')?;
    let (base, digits) = (&text[..i], &text[i + 1..]);
    if base.is_empty()
        || digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
    {
        return None;
    }
    let n: i32 = digits.parse().ok()?;
    Some((base, n.checked_add(1)?))
}

pub fn find_fname(asset: &Asset, name: &Name) -> Option<FName> {
    let map = asset.get_name_map();
    let map = map.get_ref();
    if let Some(n) = name.number {
        return map
            .search_name_reference(&name.text)
            .map(|i| map.create_fname(i, n));
    }
    if let Some((base, n)) = split_number(&name.text) {
        if let Some(i) = map.search_name_reference(base) {
            return Some(map.create_fname(i, n));
        }
    }
    map.search_name_reference(&name.text)
        .map(|i| map.create_fname(i, 0))
}

pub fn make_fname(asset: &Asset, name: &Name) -> FName {
    if let Some(f) = find_fname(asset, name) {
        return f;
    }
    let mut map = asset.get_name_map();
    let mut map = map.get_mut();
    match (name.number, split_number(&name.text)) {
        (Some(n), _) => map.add_fname_with_number(&name.text, n),
        (None, Some((base, n))) => map.add_fname_with_number(base, n),
        (None, None) => map.add_fname(&name.text),
    }
}

pub fn name_of(f: &FName) -> Name {
    Name::new(fname_text(f))
}

#[derive(Clone, Debug)]
pub struct Obj {
    pub name: String,
    pub outer: i32,
    pub class: String,
    pub class_package: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Want {
    Any,
    Function,
    Class,
    Struct,
    Export,
}

impl Want {
    fn accepts(self, i: i32, o: &Obj) -> bool {
        match self {
            Want::Any => true,
            Want::Export => i > 0,
            Want::Function => o.class.ends_with("Function"),
            Want::Class => o.class.ends_with("Class"),
            Want::Struct => o.class.ends_with("Struct"),
        }
    }

    fn default_class(self) -> Option<&'static str> {
        match self {
            Want::Function => Some("Function"),
            Want::Struct => Some("ScriptStruct"),
            _ => None,
        }
    }
}

pub struct Objects {
    pub objs: HashMap<i32, Obj>,
    by_name: HashMap<String, Vec<i32>>,
    pub first_new_import: i32,
    pub new_imports: Vec<Import>,
}

pub fn class_package_of(class: &str) -> Option<&'static str> {
    Some(match class {
        "Package"
        | "Class"
        | "Function"
        | "ScriptStruct"
        | "DelegateFunction"
        | "SparseDelegateFunction"
        | "Enum"
        | "Interface" => "/Script/CoreUObject",
        "BlueprintGeneratedClass"
        | "UserDefinedStruct"
        | "UserDefinedEnum"
        | "AnimBlueprintGeneratedClass" => "/Script/Engine",
        "WidgetBlueprintGeneratedClass" => "/Script/UMG",
        _ => return None,
    })
}

impl Objects {
    pub fn new(a: &Asset) -> Self {
        let mut objs = HashMap::new();
        for (i, imp) in a.imports.iter().enumerate() {
            objs.insert(
                -(i as i32) - 1,
                Obj {
                    name: fname_text(&imp.object_name),
                    outer: imp.outer_index.index,
                    class: imp.class_name.get_content(),
                    class_package: imp.class_package.get_content(),
                },
            );
        }
        let exports = &a.asset_data.exports;
        for (i, e) in exports.iter().enumerate() {
            let b = e.get_base_export();
            let ci = b.class_index.index;
            let class = if ci < 0 {
                a.imports
                    .get((-ci - 1) as usize)
                    .map(|x| x.object_name.get_content())
            } else if ci > 0 {
                exports
                    .get((ci - 1) as usize)
                    .map(|x| x.get_base_export().object_name.get_content())
            } else {
                None
            };
            objs.insert(
                i as i32 + 1,
                Obj {
                    name: fname_text(&b.object_name),
                    outer: b.outer_index.index,
                    class: class.unwrap_or_default(),
                    class_package: String::new(),
                },
            );
        }
        let mut by_name: HashMap<String, Vec<i32>> = HashMap::new();
        let mut keys: Vec<&i32> = objs.keys().collect();
        keys.sort();
        for k in keys {
            by_name.entry(objs[k].name.clone()).or_default().push(*k);
        }
        Objects {
            objs,
            by_name,
            first_new_import: -(a.imports.len() as i32) - 1,
            new_imports: vec![],
        }
    }

    pub fn get(&self, i: i32) -> Option<&Obj> {
        self.objs.get(&i)
    }

    pub fn name(&self, i: i32) -> String {
        self.objs
            .get(&i)
            .map(|o| o.name.clone())
            .unwrap_or_default()
    }

    pub fn chain(&self, i: i32) -> Vec<String> {
        let mut out = vec![];
        let mut cur = i;
        let mut guard = 0;
        while cur != 0 && guard < 64 {
            let Some(o) = self.objs.get(&cur) else { break };
            out.push(o.name.clone());
            cur = o.outer;
            guard += 1;
        }
        out.reverse();
        out
    }

    pub fn root(&self, i: i32) -> i32 {
        let mut cur = i;
        for _ in 0..64 {
            match self.objs.get(&cur) {
                Some(o) if o.outer != 0 => cur = o.outer,
                _ => break,
            }
        }
        cur
    }

    pub fn is_function(&self, i: i32) -> bool {
        self.objs
            .get(&i)
            .is_some_and(|o| Want::Function.accepts(i, o))
    }

    pub fn is_struct(&self, i: i32) -> bool {
        self.objs
            .get(&i)
            .is_some_and(|o| Want::Struct.accepts(i, o))
    }

    pub fn is_native(&self, i: i32) -> bool {
        i < 0 && self.name(self.root(i)).starts_with("/Script/")
    }

    pub fn path_string(&self, i: i32) -> String {
        let chain = self.chain(i);
        let mut s = String::new();
        for (k, n) in chain.iter().enumerate() {
            if k == 1 {
                s.push(if i < 0 { '.' } else { ':' });
            } else if k == 2 {
                s.push(if i < 0 { ':' } else { '.' });
            } else if k > 2 {
                s.push('.');
            }
            s.push_str(n);
        }
        s
    }

    fn by_segments(&self, segs: &[String], full: bool, import_root: Option<bool>) -> Vec<i32> {
        let Some(last) = segs.last() else {
            return vec![];
        };
        let Some(cands) = self.by_name.get(last) else {
            return vec![];
        };
        cands
            .iter()
            .copied()
            .filter(|&i| {
                if let Some(imp) = import_root {
                    if (i < 0) != imp {
                        return false;
                    }
                }
                let chain = self.chain(i);
                if full {
                    chain == segs
                } else {
                    chain.ends_with(segs)
                }
            })
            .collect()
    }

    pub fn resolve(&self, r: &Ref, want: Want) -> Vec<i32> {
        match r {
            Ref::Index(i) => {
                if *i == 0 || self.objs.contains_key(i) {
                    vec![*i]
                } else {
                    vec![]
                }
            }
            Ref::Path(segs) => {
                let segs: Vec<String> = segs.iter().map(|n| n.text.clone()).collect();
                let all = self.by_segments(&segs, false, None);
                let wanted: Vec<i32> = all
                    .iter()
                    .copied()
                    .filter(|i| want.accepts(*i, &self.objs[i]))
                    .collect();
                if wanted.is_empty() && want != Want::Export {
                    all
                } else {
                    wanted
                }
            }
            Ref::Quoted { class, path } => {
                let segs = split_path(path);
                let import = path.starts_with('/');
                self.by_segments(&segs, true, Some(import))
                    .into_iter()
                    .filter(|i| want != Want::Export || *i > 0)
                    .filter(|i| class.as_ref().is_none_or(|c| self.objs[i].class == c.text))
                    .collect()
            }
        }
    }

    pub fn resolve_one(&self, r: &Ref, want: Want) -> Result<i32, String> {
        match self.resolve(r, want).as_slice() {
            [i] => Ok(*i),
            [] => Err(format!("{r} not found")),
            many => Err(format!(
                "{r} is ambiguous ({})",
                many.iter()
                    .map(|i| self.path_string(*i))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    pub fn print_ref(&self, i: i32, want: Want) -> Ref {
        if i == 0 || !self.objs.contains_key(&i) {
            return Ref::Index(i);
        }
        let chain = self.chain(i);
        for k in 1..=chain.len() {
            let segs: Vec<Name> = chain[chain.len() - k..]
                .iter()
                .map(|s| Name::new(s.clone()))
                .collect();
            if segs[0].text.starts_with('/') {
                break;
            }
            let r = Ref::Path(segs);
            if self.resolve(&r, want) == [i] {
                return r;
            }
            if k >= 3 {
                break;
            }
        }
        let path = self.path_string(i);
        let r = Ref::Quoted {
            class: None,
            path: path.clone(),
        };
        if self.resolve(&r, want) == [i] {
            return r;
        }
        let r = Ref::Quoted {
            class: Some(Name::new(self.objs[&i].class.clone())),
            path,
        };
        if self.resolve(&r, want) == [i] {
            return r;
        }
        Ref::Index(i)
    }

    pub fn add_import(&mut self, imp: Import) -> i32 {
        let idx = self.first_new_import - self.new_imports.len() as i32;
        let o = Obj {
            name: fname_text(&imp.object_name),
            outer: imp.outer_index.index,
            class: imp.class_name.get_content(),
            class_package: imp.class_package.get_content(),
        };
        self.by_name.entry(o.name.clone()).or_default().push(idx);
        self.objs.insert(idx, o);
        self.new_imports.push(imp);
        idx
    }

    pub fn class_package_for(&self, class: &str) -> Option<String> {
        if let Some(p) = class_package_of(class) {
            return Some(p.to_string());
        }
        self.objs
            .values()
            .find(|o| o.class == class && !o.class_package.is_empty())
            .map(|o| o.class_package.clone())
    }

    /// Finds the import with these path segments or creates it with the given class. The outer
    /// object must exist already, except for a package.
    pub fn ensure(
        &mut self,
        a: &Asset,
        segs: &[String],
        class: &str,
        class_package: &str,
    ) -> Result<i32, String> {
        let found = self.by_segments(segs, true, Some(true));
        if let Some(i) = found
            .iter()
            .find(|i| self.objs[i].class == class)
            .or(found.first())
        {
            return Ok(*i);
        }
        let outer = match segs.len() {
            0 => return Err("empty path".into()),
            1 => 0,
            2 => self.ensure(a, &segs[..1], "Package", "/Script/CoreUObject")?,
            n => *self
                .by_segments(&segs[..n - 1], true, Some(true))
                .first()
                .ok_or_else(|| format!("{} is not imported", join_path(&segs[..n - 1])))?,
        };
        let imp = Import::new(
            make_fname(a, &Name::new(class_package)),
            make_fname(a, &Name::new(class)),
            PackageIndex::new(outer),
            make_fname(a, &Name::new(segs[segs.len() - 1].clone())),
            false,
        );
        Ok(self.add_import(imp))
    }

    pub fn create(&mut self, a: &Asset, r: &Ref, want: Want) -> Result<i32, String> {
        let Ref::Quoted { class, path } = r else {
            return Err(format!(
                "{r} not found; write new references as full quoted paths like '/Script/Engine.KismetSystemLibrary:PrintString'"
            ));
        };
        if !path.starts_with('/') {
            return Err(format!("{r} not found in this asset"));
        }
        let segs = split_path(path);
        let mut outer = 0;
        for k in 0..segs.len() {
            let found = self.by_segments(&segs[..=k], true, Some(true));
            let last = k + 1 == segs.len();
            let found: Vec<i32> = if last {
                found
                    .into_iter()
                    .filter(|i| class.as_ref().is_none_or(|c| self.objs[i].class == c.text))
                    .collect()
            } else {
                found
            };
            if let Some(i) = found.first() {
                outer = *i;
                continue;
            }
            let cdo_class = segs[k].strip_prefix("Default__").filter(|_| k == 1);
            let class_name = if k == 0 {
                "Package".to_string()
            } else if let (Some(c), None) = (cdo_class, class) {
                c.to_string()
            } else if last {
                let under_class = k >= 2
                    && self
                        .objs
                        .get(&outer)
                        .is_some_and(|o| o.class.ends_with("Class"));
                match (class, want.default_class()) {
                    (Some(c), _) => c.text.clone(),
                    (None, Some(c)) => c.to_string(),
                    (None, None) if under_class => "Function".to_string(),
                    (None, None) if segs[0].starts_with("/Script/") && k == 1 => {
                        "Class".to_string()
                    }
                    _ => {
                        return Err(format!(
                            "cannot tell the class of {}; write it as ClassName'{path}'",
                            segs[k]
                        ))
                    }
                }
            } else if segs[0].starts_with("/Script/") && k == 1 {
                "Class".to_string()
            } else {
                return Err(format!(
                    "{} is not imported yet; import it first with its class, like import BlueprintGeneratedClass'{}'",
                    segs[k],
                    join_path(&segs[..=k])
                ));
            };
            let package = match cdo_class {
                Some(_) if class.is_none() && segs[0].starts_with("/Script/") => {
                    Some(segs[0].clone())
                }
                _ => self.class_package_for(&class_name),
            };
            let package = package.ok_or_else(|| {
                format!(
                    "unknown class {class_name}; no import of that class to copy its package from"
                )
            })?;
            let imp = Import::new(
                make_fname(a, &Name::new(package)),
                make_fname(a, &Name::new(class_name)),
                PackageIndex::new(outer),
                make_fname(a, &Name::new(segs[k].clone())),
                false,
            );
            outer = self.add_import(imp);
        }
        Ok(outer)
    }
}

pub fn split_path(path: &str) -> Vec<String> {
    path.split(['.', ':'])
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn join_path(segs: &[String]) -> String {
    let mut s = String::new();
    for (k, n) in segs.iter().enumerate() {
        match k {
            0 => {}
            1 => s.push('.'),
            2 => s.push(':'),
            _ => s.push('.'),
        }
        s.push_str(n);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_split_like_unreal() {
        assert_eq!(split_number("Foo_5"), Some(("Foo", 6)));
        assert_eq!(split_number("Foo_0"), Some(("Foo", 1)));
        assert_eq!(split_number("Foo_05"), None);
        assert_eq!(split_number("Foo_"), None);
        assert_eq!(split_number("_5"), None);
        assert_eq!(split_number("Foo"), None);
        assert_eq!(split_number("A_B_12"), Some(("A_B", 13)));
    }

    #[test]
    fn paths_split_on_separators() {
        assert_eq!(
            split_path("/Script/Engine.KismetSystemLibrary:PrintString"),
            vec!["/Script/Engine", "KismetSystemLibrary", "PrintString"]
        );
        assert_eq!(
            join_path(&split_path("/Game/M.M:Level.Actor")),
            "/Game/M.M:Level.Actor"
        );
    }
}
