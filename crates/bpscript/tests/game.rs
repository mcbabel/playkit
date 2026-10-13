use bpscript::{assemble, disassemble, load, save, Asset, Files};
use std::path::PathBuf;

const DEFAULT_CONTENT: &str = "/home/marlian/.local/share/Steam/steamapps/common/Poppy Playtime - Multiplayer/pakex/orig/Playtime_Multiplayer/Content";

fn content() -> Option<PathBuf> {
    let dir = std::env::var("BPSCRIPT_GAME_CONTENT").unwrap_or_else(|_| DEFAULT_CONTENT.into());
    let p = PathBuf::from(dir);
    if p.join("Interface/MatchMaking/UI_MatchMakingScreen.uasset")
        .exists()
    {
        Some(p)
    } else {
        eprintln!("game assets not found, skipped (set BPSCRIPT_GAME_CONTENT)");
        None
    }
}

fn read(dir: &std::path::Path, rel: &str) -> Files {
    Files::read(&dir.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn real_assets_round_trip() {
    let Some(dir) = content() else { return };
    for rel in [
        "Interface/MatchMaking/UI_MatchMakingScreen.uasset",
        "Base/BP_MultiplayerPlayerController.uasset",
        "Base/BP_MultiplayerPlayerState.uasset",
        "Interface/MainMenu/UI_MainMenu.uasset",
    ] {
        let f = read(&dir, rel);
        let a = load(&f).unwrap();
        let text = disassemble(&a).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let mut b = load(&f).unwrap();
        assemble(&mut b, &text).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let out = save(&b).unwrap();
        assert!(
            out.uasset == f.uasset && out.uexp == f.uexp,
            "{rel} changed"
        );
    }
}

#[test]
fn edited_text_creates_imports() {
    let Some(dir) = content() else { return };
    let f = read(&dir, "Interface/MatchMaking/UI_MatchMakingScreen.uasset");
    let a = load(&f).unwrap();
    let text = disassemble(&a).unwrap();
    let edited = text.replacen(
        "function Construct {\n",
        "function Construct {\n    '/Script/Engine.Default__KismetSystemLibrary'.PrintString(self, \"hi\", true, true, LinearColor{1.0, 0.0, 0.0, 1.0}, 2.0);\n    '/Script/Engine.KismetSystemLibrary:K2_SetTimer'(self, \"Later\", 1.0, false);\n",
        1,
    );
    assert_ne!(text, edited);
    let mut b = load(&f).unwrap();
    assemble(&mut b, &edited).unwrap();
    assert_eq!(b.imports.len(), a.imports.len() + 2);
    let again = disassemble(&load(&save(&b).unwrap()).unwrap()).unwrap();
    assert!(again.contains("Default__KismetSystemLibrary.PrintString(self, \"hi\""));
    assert!(again.contains("    K2_SetTimer(self, \"Later\", 1.0, false);"));
}

fn function_text<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text
        .find(&format!("function {name} {{"))
        .unwrap_or_else(|| panic!("function {name} missing"));
    let end = text[start..].find("\n}\n").expect("function end") + start;
    &text[start..end]
}

const SECOND_MOD: &str = r#"
var PM_TestCounter: int;

before Construct {
    PM_TestCounter = 1;
}

after SwitchCameras {
    local Note: string;
    Note = "cameras switched";
}

event PM_TestEvent(Value: int) {
    PM_TestCounter = Value;
}
"#;

fn patched(base: &Files, src: &str) -> Files {
    let mut a = load(base).unwrap();
    bpscript::patch::apply(&mut a, src).unwrap();
    save(&a).unwrap()
}

fn check_merged(merged: &Files) -> Asset {
    let a = load(merged).unwrap();
    let text = disassemble(&a).unwrap();
    for f in [
        "PM_InviteClick",
        "PM_InviteCmd",
        "PM_InviteRequest",
        "PM_InviteJoined",
        "PM_TestEvent",
    ] {
        function_text(&text, f);
    }
    let construct = function_text(&text, "Construct");
    assert!(construct.contains("PM_TestCounter = 1;"), "{construct}");
    assert!(
        construct
            .matches("ExecuteUbergraph_UI_MatchMakingScreen(@L")
            .count()
            == 2,
        "{construct}"
    );
    let uber = function_text(&text, "ExecuteUbergraph_UI_MatchMakingScreen");
    assert!(uber.contains("bind(PM_InvDelBtn, self, PM_InviteClick);"));
    assert!(uber.contains("PM_TestCounter = PM_TestEvent_Value;"));
    assert!(function_text(&text, "SwitchCameras").contains("Note = \"cameras switched\";"));
    let mut b = load(merged).unwrap();
    assemble(&mut b, &text).unwrap();
    let out = save(&b).unwrap();
    assert!(out.uasset == merged.uasset && out.uexp == merged.uexp);
    a
}

#[test]
fn patches_merge_with_each_other() {
    let Some(dir) = content() else { return };
    let base = read(&dir, "Interface/MatchMaking/UI_MatchMakingScreen.uasset");
    let invite = include_str!("../examples/invite.bps");
    let a = patched(&base, invite);
    let b = patched(&base, SECOND_MOD);
    let to_merge = |f: &Files| playkit_merge::Files {
        uasset: f.uasset.clone(),
        uexp: f.uexp.clone(),
    };
    let base_m = to_merge(&base);
    for mods in [[&a, &b], [&b, &a]] {
        let merged = playkit_merge::merge(&base_m, &[to_merge(mods[0]), to_merge(mods[1])])
            .expect("merge accepts both patches");
        let merged = Files {
            uasset: merged.uasset,
            uexp: merged.uexp,
        };
        check_merged(&merged);
        let back = playkit_merge::Files {
            uasset: merged.uasset.clone(),
            uexp: merged.uexp.clone(),
        };
        assert!(playkit_merge::contains(&base_m, &to_merge(&a), &back).unwrap());
        assert!(playkit_merge::contains(&base_m, &to_merge(&b), &back).unwrap());
    }
}
