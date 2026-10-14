//! Use of the game's reflection database (`playkit-sdk/reflection.json`) for names that the
//! asset itself cannot resolve.

use crate::ast::{CallKind, LetKw};
pub use playkit_reflect::{short_name, Db, Enum, Func, Index, Prop, Struct, Type};
use std::path::{Path, PathBuf};

pub const ENV_VAR: &str = "PLAYKIT_REFLECTION";

const FUNC_FINAL: u32 = 0x1;
const FUNC_NET: u32 = 0x40 | 0x80 | 0x4000 | 0x20_0000 | 0x100_0000;
const FUNC_AUTHORITY_COSMETIC: u32 = 0x4 | 0x8;

pub fn load(path: &Path) -> Result<Db, String> {
    Db::load(path).map_err(|e| format!("reflection database {e}"))
}

/// The database given with `--reflection`, otherwise the one named by `PLAYKIT_REFLECTION`.
pub fn path_from(arg: Option<&str>) -> Option<PathBuf> {
    match arg {
        Some(p) => Some(PathBuf::from(p)),
        None => std::env::var_os(ENV_VAR)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from),
    }
}

/// Native functions with wildcard parameters (`CustomThunk`). The compiler never calls them
/// with `CallMath`; static ones go through the class default object.
fn custom_thunk(owner: &Struct, f: &Func) -> bool {
    let class = short_name(&owner.path);
    let n = f.name.as_str();
    match class {
        "KismetArrayLibrary" => n.starts_with("Array_") || n == "SetArrayPropertyByName",
        "BlueprintMapLibrary" => n.starts_with("Map_") || n == "SetMapPropertyByName",
        "BlueprintSetLibrary" => n.starts_with("Set_") || n == "SetSetPropertyByName",
        "DataTableFunctionLibrary" => n == "GetDataTableRowFromName",
        _ => f
            .params
            .iter()
            .any(|p| p.is_param() && p.ty.contains("/Script/Engine.GenericStruct")),
    }
}

/// The call token the Kismet compiler emits for `f`, checked against the bytecode of the game.
pub fn call_kind(db: &Index, owner: &Struct, f: &Func) -> CallKind {
    let flags = f.flags;
    let interface = db.is_child_of(owner, "/Script/CoreUObject.Interface");
    if f.is_static()
        && f.is_native()
        && flags & FUNC_FINAL != 0
        && flags & FUNC_AUTHORITY_COSMETIC == 0
        && !interface
        && !custom_thunk(owner, f)
    {
        return CallKind::Math;
    }
    let fin = flags & FUNC_FINAL != 0;
    let local = !f.is_native() && flags & FUNC_NET == 0;
    match (fin, local) {
        (true, true) => CallKind::LocalFinal,
        (true, false) => CallKind::Final,
        (false, true) => CallKind::LocalVirtual,
        (false, false) => CallKind::Virtual,
    }
}

/// Assignment token for a property of this type.
pub fn let_kw(ty: &str) -> LetKw {
    match Type::parse(ty).kind() {
        "BoolProperty" => LetKw::Bool,
        "ObjectProperty" | "ClassProperty" => LetKw::Obj,
        "WeakObjectProperty" => LetKw::Weak,
        "DelegateProperty" => LetKw::Delegate,
        "MulticastInlineDelegateProperty" | "MulticastSparseDelegateProperty" => LetKw::Multicast,
        _ => LetKw::Let,
    }
}

/// Class or struct that a value of this property type points to or contains.
pub fn type_target<'a>(db: &Index<'a>, ty: &str) -> Option<&'a Struct> {
    match Type::parse(ty) {
        Type::Ref(kind, path)
            if matches!(
                kind.as_str(),
                "ObjectProperty"
                    | "Struct"
                    | "InterfaceProperty"
                    | "WeakObjectProperty"
                    | "LazyObjectProperty"
                    | "SoftObjectProperty"
            ) =>
        {
            db.get(&path)
        }
        _ => None,
    }
}

/// Class and class package of an import of `s`.
pub fn import_class(db: &Index, s: &Struct) -> (&'static str, &'static str) {
    let native = s.path.starts_with("/Script/");
    match (s.kind == "class", native) {
        (true, true) => ("Class", "/Script/CoreUObject"),
        (false, true) => ("ScriptStruct", "/Script/CoreUObject"),
        (false, false) => ("UserDefinedStruct", "/Script/Engine"),
        (true, false) if db.is_child_of(s, "/Script/UMG.UserWidget") => {
            ("WidgetBlueprintGeneratedClass", "/Script/UMG")
        }
        (true, false) if db.is_child_of(s, "/Script/Engine.AnimInstance") => {
            ("AnimBlueprintGeneratedClass", "/Script/Engine")
        }
        (true, false) => ("BlueprintGeneratedClass", "/Script/Engine"),
    }
}

/// Class of a function import with this path.
pub fn function_class(path: &str) -> &'static str {
    if path.starts_with("/Script/") && path.ends_with("__DelegateSignature") {
        "DelegateFunction"
    } else {
        "Function"
    }
}

/// `; did you mean A, B?` or nothing.
pub fn suggest(names: &[&str]) -> String {
    if names.is_empty() {
        String::new()
    } else {
        format!("; did you mean {}?", names.join(", "))
    }
}

/// Parameters a caller passes, without the return value.
pub fn call_params(f: &Func) -> Vec<&Prop> {
    f.params().filter(|p| !p.is_return()).collect()
}

pub fn return_param(f: &Func) -> Option<&Prop> {
    f.params().find(|p| p.is_return())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn fixture() -> Db {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reflection_small.json");
        load(&path).expect("fixture database")
    }

    fn kind(db: &Index, class: &str, f: &str) -> CallKind {
        let c = db.get(class).expect(class);
        let (owner, func) = db.function(c, f).expect(f);
        call_kind(db, owner, func)
    }

    #[test]
    fn call_kinds_follow_the_compiler() {
        let db = fixture();
        let i = db.index();
        let lib = "/Script/Engine.KismetSystemLibrary";
        assert_eq!(kind(&i, lib, "PrintString"), CallKind::Math);
        assert_eq!(kind(&i, lib, "K2_SetTimer"), CallKind::Math);
        assert_eq!(kind(&i, lib, "IsValid"), CallKind::Math);
        assert_eq!(kind(&i, lib, "SetStructurePropertyByName"), CallKind::Final);
        let arr = "/Script/Engine.KismetArrayLibrary";
        assert_eq!(kind(&i, arr, "Array_Get"), CallKind::Final);
        let wbl = "/Script/UMG.WidgetBlueprintLibrary";
        assert_eq!(kind(&i, wbl, "GetAllWidgetsOfClass"), CallKind::Final);
        let w = "/Script/UMG.Widget";
        assert_eq!(kind(&i, w, "SetVisibility"), CallKind::Virtual);
        assert_eq!(kind(&i, w, "GetParent"), CallKind::Final);
        let bp = "/Game/Interface/MainMenu/Widgets/UI_InvitePlayerButton.UI_InvitePlayerButton_C";
        assert_eq!(kind(&i, bp, "HidePlayerIcon"), CallKind::LocalVirtual);
        assert_eq!(kind(&i, bp, "Server_Ping"), CallKind::Virtual);
        assert_eq!(kind(&i, bp, "GetParent"), CallKind::Final);
    }

    #[test]
    fn types_and_imports() {
        let db = fixture();
        let i = db.index();
        assert_eq!(let_kw("BoolProperty"), LetKw::Bool);
        assert_eq!(let_kw("ObjectProperty</Script/UMG.Widget>"), LetKw::Obj);
        assert_eq!(let_kw("ClassProperty</Script/UMG.UserWidget>"), LetKw::Obj);
        assert_eq!(let_kw("Struct</Script/SlateCore.Margin>"), LetKw::Let);
        assert_eq!(
            let_kw("DelegateProperty</Script/A.B__DelegateSignature>"),
            LetKw::Delegate
        );
        assert_eq!(
            let_kw("MulticastInlineDelegateProperty</Script/A.B__DelegateSignature>"),
            LetKw::Multicast
        );
        let target = |t: &str| type_target(&i, t).map(|s| s.path.as_str());
        assert_eq!(
            target("ObjectProperty</Script/UMG.Widget>"),
            Some("/Script/UMG.Widget")
        );
        assert_eq!(
            target("Struct</Script/SlateCore.Margin>"),
            Some("/Script/SlateCore.Margin")
        );
        assert_eq!(
            target("ArrayProperty<ObjectProperty</Script/UMG.Widget>>"),
            None
        );
        assert_eq!(target("IntProperty"), None);
        let class = |p: &str| import_class(&i, i.get(p).expect(p));
        assert_eq!(
            class("/Script/UMG.Widget"),
            ("Class", "/Script/CoreUObject")
        );
        assert_eq!(
            class("/Script/SlateCore.Margin"),
            ("ScriptStruct", "/Script/CoreUObject")
        );
        assert_eq!(
            class("/Game/Base/S_Test.S_Test"),
            ("UserDefinedStruct", "/Script/Engine")
        );
        assert_eq!(
            class("/Game/Interface/MainMenu/Widgets/UI_InvitePlayerButton.UI_InvitePlayerButton_C"),
            ("WidgetBlueprintGeneratedClass", "/Script/UMG")
        );
        assert_eq!(
            class("/Game/Anim/ABP_Test.ABP_Test_C"),
            ("AnimBlueprintGeneratedClass", "/Script/Engine")
        );
        assert_eq!(
            function_class("/Script/SteamCore.OnGameLobbyJoinRequested__DelegateSignature"),
            "DelegateFunction"
        );
        assert_eq!(
            function_class("/Game/A.B_C:Done__DelegateSignature"),
            "Function"
        );
        assert_eq!(
            function_class("/Script/UMG.Widget:SetVisibility"),
            "Function"
        );
        let split = i
            .function(
                i.get("/Script/Engine.KismetStringLibrary").unwrap(),
                "Split",
            )
            .unwrap()
            .1;
        assert_eq!(call_params(split).len(), 6);
        assert_eq!(
            return_param(split).map(|p| p.ty.as_str()),
            Some("BoolProperty")
        );
        assert_eq!(suggest(&[]), "");
        assert_eq!(suggest(&["A", "B"]), "; did you mean A, B?");
        assert_eq!(path_from(Some("x.json")), Some(PathBuf::from("x.json")));
    }
}
