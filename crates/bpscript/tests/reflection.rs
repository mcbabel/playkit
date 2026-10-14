//! Reflection database tests on real game assets: a small fixture database for the
//! resolution rules, the game's own database for the examples and round trips. Skipped when the
//! game folder is missing.

use bpscript::lower::{dest_ptr, pkey, struct_parts, KE};
use bpscript::reflection::{load, Db};
use bpscript::{assemble_with, disassemble, load as load_asset, save, Asset, Files};
use std::path::{Path, PathBuf};
use unreal_asset::kismet::walk::{Visitor, Walk};

const DEFAULT_GAME: &str =
    "/home/marlian/.local/share/Steam/steamapps/common/Poppy Playtime - Multiplayer";
const SCREEN: &str = "Interface/MatchMaking/UI_MatchMakingScreen.uasset";

fn game() -> PathBuf {
    PathBuf::from(std::env::var("BPSCRIPT_GAME_DIR").unwrap_or_else(|_| DEFAULT_GAME.into()))
}

fn content() -> Option<PathBuf> {
    let p = match std::env::var("BPSCRIPT_GAME_CONTENT") {
        Ok(c) => PathBuf::from(c),
        Err(_) => game().join("pakex/orig/Playtime_Multiplayer/Content"),
    };
    if p.join(SCREEN).exists() {
        Some(p)
    } else {
        eprintln!("game assets not found, skipped (set BPSCRIPT_GAME_CONTENT)");
        None
    }
}

fn game_db() -> Option<Db> {
    let p = match std::env::var("PLAYKIT_REFLECTION") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => game().join("Playtime_Multiplayer/Binaries/Win64/playkit-sdk/reflection.json"),
    };
    if p.exists() {
        Some(load(&p).expect("game reflection database"))
    } else {
        eprintln!("game reflection database not found, skipped (set PLAYKIT_REFLECTION)");
        None
    }
}

fn fixture() -> Db {
    load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reflection_small.json"))
        .expect("fixture database")
}

fn read(path: &Path) -> Files {
    Files::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn patch(base: &Files, src: &str, db: Option<&Db>) -> Result<(Asset, String), String> {
    let index = db.map(Db::index);
    let mut a = load_asset(base).unwrap();
    let report =
        bpscript::patch::apply_with(&mut a, src, index.as_ref()).map_err(|e| e.to_string())?;
    Ok((a, report))
}

fn reload(a: &Asset) -> (Files, Asset) {
    let f = save(a).unwrap();
    let back = load_asset(&f).unwrap();
    (f, back)
}

fn round_trips(f: &Files, a: &Asset) {
    let text = disassemble(a).unwrap();
    let mut b = load_asset(f).unwrap();
    assemble_with(&mut b, &text, None).unwrap();
    let out = save(&b).unwrap();
    assert!(
        out.uasset == f.uasset && out.uexp == f.uexp,
        "patched asset does not round-trip"
    );
}

#[derive(Default)]
struct Tokens(Vec<String>);

impl Visitor for Tokens {
    fn expr(&mut self, e: &mut KE) {
        let t = match e {
            KE::ExLetBool(l) => match dest_ptr(&l.variable_expression).and_then(pkey) {
                Some((_, n)) => format!("letbool {n}"),
                None => return,
            },
            KE::ExVirtualFunction(v) => {
                format!("virtual {}", v.virtual_function_name.get_content())
            }
            KE::ExLocalVirtualFunction(v) => {
                format!("localvirtual {}", v.virtual_function_name.get_content())
            }
            _ => return,
        };
        self.0.push(t);
    }
}

fn tokens(a: &Asset) -> Vec<String> {
    let mut t = Tokens::default();
    for e in &a.asset_data.exports {
        if let Some(code) = struct_parts(e).and_then(|s| s.script_bytecode.clone()) {
            for mut x in code {
                x.walk(&mut t);
            }
        }
    }
    t.0
}

fn imports(a: &Asset) -> Vec<String> {
    a.imports
        .iter()
        .map(|i| {
            format!(
                "{} {} {}",
                i.object_name.get_content(),
                i.class_name.get_content(),
                i.class_package.get_content()
            )
        })
        .collect()
}

const SHORT: &str = r#"
event PM_T() {
    local PM_S: string;
    local PM_Menus: array<object<UserWidget>>;
    local PM_Menu: object<UserWidget>;
    KismetSystemLibrary::PrintString(self, "x", true, true, LinearColor{1.0, 0.0, 0.0, 1.0}, 2.0);
    PM_S = GetCommandLine();
    LaunchURL(PM_S);
    SlotAsOverlaySlot(InvitePlayer1.GetParent()).SetPadding(Margin{1.0, 2.0, 3.0, 4.0});
    InvitePlayer1.GetParent().SetRenderScale(Vector2D{2.0, 2.0});
    InvitePlayer1.SetVisibility(ESlateVisibility::Collapsed);
    InvitePlayer1.bIsEnabled = false;
    GetAllWidgetsOfClass(self, PM_Menus, UserWidget, false);
    Array_Get(PM_Menus, 0, PM_Menu);
    InvitePlayer1.Server_Ping();
}
"#;

#[test]
fn fixture_db_resolves_short_names() {
    let Some(dir) = content() else { return };
    let base = read(&dir.join(SCREEN));
    let e = patch(&base, SHORT, None).unwrap_err();
    assert!(e.contains("Margin not found"), "{e}");
    let db = fixture();
    let (a, report) = patch(&base, SHORT, Some(&db)).unwrap();
    assert!(
        report.contains("import SlotAsOverlaySlot (Function)"),
        "{report}"
    );
    let (f, a2) = reload(&a);
    let text = disassemble(&a2).unwrap();
    for line in [
        "PrintString(self, \"x\", true, true, LinearColor{1.0, 0.0, 0.0, 1.0}, 2.0);",
        "PM_S = GetCommandLine();",
        "LaunchURL(PM_S);",
        "SlotAsOverlaySlot(InvitePlayer1.GetParent()).SetPadding(Margin{1.0, 2.0, 3.0, 4.0});",
        "InvitePlayer1.GetParent().SetRenderScale(Vector2D{2.0, 2.0});",
        "InvitePlayer1.SetVisibility(1u8);",
        "InvitePlayer1.bIsEnabled = false;",
        "Default__WidgetBlueprintLibrary.GetAllWidgetsOfClass(self, PM_Menus, UserWidget, false);",
        "Default__KismetArrayLibrary.Array_Get(PM_Menus, 0, PM_Menu);",
        "InvitePlayer1.Server_Ping();",
    ] {
        assert!(text.contains(line), "missing: {line}");
    }
    let t = tokens(&a2);
    for tok in [
        "letbool bIsEnabled",
        "virtual SetVisibility",
        "virtual Server_Ping",
    ] {
        assert!(t.iter().any(|x| x == tok), "missing {tok}");
    }
    let imps = imports(&a2);
    for imp in [
        "SlotAsOverlaySlot Function /Script/CoreUObject",
        "WidgetLayoutLibrary Class /Script/CoreUObject",
        "Default__WidgetBlueprintLibrary WidgetBlueprintLibrary /Script/UMG",
        "Default__KismetArrayLibrary KismetArrayLibrary /Script/Engine",
    ] {
        assert!(imps.iter().any(|x| x == imp), "missing import {imp}");
    }
    round_trips(&f, &a2);
}

#[test]
fn fixture_db_errors_name_the_problem() {
    let Some(dir) = content() else { return };
    let base = read(&dir.join(SCREEN));
    let db = fixture();
    let err = |stmt: &str| {
        let src = format!("event PM_T() {{\n    {stmt}\n}}\n");
        patch(&base, &src, Some(&db)).unwrap_err()
    };
    let cases = [
        ("PrintStrng(self);", "function PrintStrng not found"),
        ("PrintStrng(self);", "did you mean PrintString"),
        (
            "InvitePlayer1.SetVisibilty(ESlateVisibility::Visible);",
            "UI_InvitePlayerButton_C has no function SetVisibilty; did you mean SetVisibility",
        ),
        (
            "InvitePlayer1.SetVisibility(ESlateVisibility::Colapsed);",
            "ESlateVisibility has no value Colapsed; values: Visible, Collapsed",
        ),
        (
            "InvitePlayer1.SetRenderScale(Vector2X{1.0, 1.0});",
            "Vector2X not found in the asset or the reflection database; did you mean Vector2D",
        ),
        (
            "InvitePlayer1.bIsEnabld = true;",
            "UI_InvitePlayerButton_C has no member bIsEnabld; did you mean bIsEnabled",
        ),
        ("PM_Dup();", "PM_Dup is ambiguous"),
        (
            "PM_Dup();",
            "KismetSystemLibrary::PM_Dup, SteamUtilities::PM_Dup",
        ),
        (
            "LaunchURL();",
            "KismetSystemLibrary::LaunchURL takes 1 arguments (URL), not 0",
        ),
        ("GetParnt();", "did you mean GetParent"),
    ];
    for (stmt, want) in cases {
        let e = err(stmt);
        assert!(e.contains(want), "{stmt}: {e}");
    }
    let src = "event PM_T() {\n    KismetSystemLibrary::PM_Dup();\n}\n";
    patch(&base, src, Some(&db)).unwrap();
}

#[test]
fn game_db_round_trips_with_and_without_inference() {
    let (Some(dir), Some(db)) = (content(), game_db()) else {
        return;
    };
    let index = db.index();
    for rel in [
        SCREEN,
        "Base/BP_MultiplayerPlayerController.uasset",
        "Base/BP_MultiplayerPlayerState.uasset",
        "Interface/MainMenu/UI_MainMenu.uasset",
    ] {
        let f = read(&dir.join(rel));
        let a = load_asset(&f).unwrap();
        let plain = disassemble(&a).unwrap();
        let short = bpscript::print::disassemble_with(&a, Some(&index)).unwrap();
        assert!(short.len() <= plain.len(), "{rel}");
        for text in [&plain, &short] {
            let mut b = load_asset(&f).unwrap();
            assemble_with(&mut b, text, Some(&index)).unwrap_or_else(|e| panic!("{rel}: {e}"));
            let out = save(&b).unwrap();
            assert!(
                out.uasset == f.uasset && out.uexp == f.uexp,
                "{rel} changed"
            );
        }
    }
}

fn function_text<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text
        .find(&format!("function {name} {{"))
        .unwrap_or_else(|| panic!("function {name} missing"));
    let end = text[start..].find("\n}\n").expect("function end") + start;
    &text[start..end]
}

#[test]
fn game_db_invite_short_matches_invite() {
    let (Some(dir), Some(db)) = (content(), game_db()) else {
        return;
    };
    let base = read(&dir.join(SCREEN));
    let full = include_str!("../examples/invite.bps");
    let short = include_str!("../examples/invite_short.bps");
    let (a, _) = patch(&base, full, Some(&db)).unwrap();
    let (b, _) = patch(&base, short, Some(&db)).unwrap();
    let (_, a) = reload(&a);
    let (fb, b) = reload(&b);
    assert_eq!(disassemble(&a).unwrap(), disassemble(&b).unwrap());
    let mut ia = imports(&a);
    let mut ib = imports(&b);
    ia.sort();
    ib.sort();
    ia.retain(|x| !ib.contains(x));
    assert_eq!(
        ia,
        ["SetVisibility Function /Script/CoreUObject"],
        "unused import of invite.bps"
    );
    assert!(imports(&b).iter().all(|x| imports(&a).contains(x)));
    let text = disassemble(&b).unwrap();
    for f in [
        "PM_InviteClick",
        "PM_InviteCmd",
        "PM_InviteRequest",
        "PM_InviteJoined",
    ] {
        function_text(&text, f);
    }
    round_trips(&fb, &b);
}

#[test]
fn game_db_picks_overlay_slot_on_base_with_vertical_box_slot() {
    let Some(db) = game_db() else { return };
    let path = game().join("pakex/pm49/vorher").join(SCREEN);
    if !path.exists() {
        eprintln!("{} not found, skipped", path.display());
        return;
    }
    let base = read(&path);
    let before = load_asset(&base).unwrap();
    assert!(imports(&before)
        .iter()
        .any(|x| x.starts_with("SetPadding Function")));
    let short = include_str!("../examples/invite_short.bps");
    let (a, _) = patch(&base, short, Some(&db)).unwrap();
    let (_, a) = reload(&a);
    let text = disassemble(&a).unwrap();
    assert!(
        text.contains("SlotAsOverlaySlot(InvitePlayer1.GetParent()).OverlaySlot::SetPadding(Margin{72.0, 100.0, 0.0, 0.0});"),
        "SetPadding must be the OverlaySlot one"
    );
}
