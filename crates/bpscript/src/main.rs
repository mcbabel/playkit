use bpscript::reflection::{self, Db, Index};
use bpscript::{assemble_with, disassemble, load, save, Files};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  bpscript dis <asset.uasset|.umap> [out.bps]
  bpscript asm <asset> <in.bps> <out.uasset>
  bpscript patch <in.uasset> <mod.bps> <out.uasset>
  bpscript roundtrip <dir> [--limit N] [--keep DIR]
options:
  --reflection <file>  reflection database (playkit-sdk/reflection.json) for names the
                       asset does not contain; default: env PLAYKIT_REFLECTION";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut db_arg = None;
    if let Some(k) = args.iter().position(|a| a == "--reflection") {
        if k + 1 >= args.len() {
            eprintln!("--reflection needs a file\n{USAGE}");
            return ExitCode::FAILURE;
        }
        db_arg = Some(args.remove(k + 1));
        args.remove(k);
    }
    let needs_db = matches!(
        args.first().map(String::as_str),
        Some("asm" | "patch" | "roundtrip")
    );
    let db = match reflection::path_from(db_arg.as_deref()).filter(|_| needs_db) {
        Some(p) => match reflection::load(&p) {
            Ok(db) => Some(db),
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let index = db.as_ref().map(Db::index);
    let db = index.as_ref();
    let r = match args.first().map(String::as_str) {
        Some("dis") if args.len() == 2 || args.len() == 3 => dis(&args[1], args.get(2)),
        Some("asm") if args.len() == 4 => asm(&args[1], &args[2], &args[3], db),
        Some("patch") if args.len() == 4 => patch(&args[1], &args[2], &args[3], db),
        Some("roundtrip") if args.len() >= 2 => roundtrip(&args[1..], db),
        _ => Err(USAGE.to_string()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn read(path: &str) -> Result<Files, String> {
    Files::read(Path::new(path)).map_err(|e| format!("{path}: {e}"))
}

fn dis(input: &str, out: Option<&String>) -> Result<(), String> {
    let a = load(&read(input)?).map_err(|e| e.to_string())?;
    let text = disassemble(&a)?;
    match out {
        Some(o) => std::fs::write(o, text).map_err(|e| format!("{o}: {e}")),
        None => {
            print!("{text}");
            Ok(())
        }
    }
}

fn write_out(a: &bpscript::Asset, out: &str) -> Result<(), String> {
    let f = save(a).map_err(|e| e.to_string())?;
    f.write(Path::new(out)).map_err(|e| format!("{out}: {e}"))
}

fn asm(input: &str, src: &str, out: &str, db: Option<&Index>) -> Result<(), String> {
    let mut a = load(&read(input)?).map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(src).map_err(|e| format!("{src}: {e}"))?;
    let n = assemble_with(&mut a, &text, db).map_err(|e| format!("{src}: {e}"))?;
    write_out(&a, out)?;
    println!("{n} functions assembled; written: {out}");
    Ok(())
}

fn patch(input: &str, src: &str, out: &str, db: Option<&Index>) -> Result<(), String> {
    let mut a = load(&read(input)?).map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(src).map_err(|e| format!("{src}: {e}"))?;
    let report =
        bpscript::patch::apply_with(&mut a, &text, db).map_err(|e| format!("{src}: {e}"))?;
    write_out(&a, out)?;
    print!("{report}");
    println!("written: {out}");
    Ok(())
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect(&p, out);
        } else if matches!(
            p.extension().and_then(|x| x.to_str()),
            Some("uasset" | "umap")
        ) {
            out.push(p);
        }
    }
}

enum Outcome {
    Unreadable(String),
    NotResaved,
    RawCode,
    NoCode,
    Same,
    Failed(String),
}

fn has_code(a: &bpscript::Asset) -> (bool, bool) {
    let (mut code, mut raw) = (false, false);
    for e in &a.asset_data.exports {
        if let Some(s) = bpscript::lower::struct_parts(e) {
            match (&s.script_bytecode, &s.script_bytecode_raw) {
                (Some(c), _) if !c.is_empty() => code = true,
                (None, Some(r)) if !r.is_empty() => raw = true,
                _ => {}
            }
        }
    }
    (code, raw)
}

fn first_difference(orig: &bpscript::Asset, new: &bpscript::Asset) -> String {
    for f in bpscript::print::functions(orig) {
        let n = new
            .asset_data
            .exports
            .get((f.index - 1) as usize)
            .and_then(bpscript::lower::struct_parts)
            .and_then(|s| s.script_bytecode.clone())
            .unwrap_or_default();
        if n != f.code {
            let k = f.code.iter().zip(&n).position(|(x, y)| x != y);
            let name = bpscript::names::fname_text(
                &unreal_asset::exports::ExportBaseTrait::get_base_export(
                    &orig.asset_data.exports[(f.index - 1) as usize],
                )
                .object_name,
            );
            return match k {
                Some(k) => format!(
                    "{name}: statement {k} differs\n      was {:?}\n      now {:?}",
                    f.code[k], n[k]
                ),
                None => format!("{name}: {} statements, now {}", f.code.len(), n.len()),
            };
        }
    }
    if orig.imports.len() != new.imports.len() {
        return "imports changed".into();
    }
    "bytes differ outside the bytecode".into()
}

fn check(path: &Path, rel: &str, keep: Option<&Path>, db: Option<&Index>) -> Outcome {
    let files = match Files::read(path) {
        Ok(f) => f,
        Err(e) => return Outcome::Unreadable(e.to_string()),
    };
    let orig = match catch_unwind(|| load(&files)) {
        Ok(Ok(a)) => a,
        Ok(Err(e)) => return Outcome::Unreadable(e.to_string()),
        Err(_) => return Outcome::Unreadable("unreal_asset panicked".into()),
    };
    let (code, raw) = has_code(&orig);
    match catch_unwind(AssertUnwindSafe(|| save(&orig))) {
        Ok(Ok(f)) if f.uasset == files.uasset && f.uexp == files.uexp => {}
        _ if code || raw => return Outcome::NotResaved,
        _ => return Outcome::NoCode,
    }
    if raw {
        return Outcome::RawCode;
    }
    if !code {
        return Outcome::NoCode;
    }
    let run = || -> Result<Option<String>, String> {
        let text = disassemble(&orig).map_err(|e| format!("dis: {e}"))?;
        if let Some(k) = keep {
            let name = Path::new(rel).with_extension("bps");
            let name = name.to_string_lossy().replace(['/', '\\'], ".");
            let _ = std::fs::write(k.join(name), &text);
        }
        let mut a = load(&files).map_err(|e| e.to_string())?;
        assemble_with(&mut a, &text, db).map_err(|e| format!("asm: {e}"))?;
        let out = save(&a).map_err(|e| format!("save: {e}"))?;
        if out.uasset == files.uasset && out.uexp == files.uexp {
            return Ok(None);
        }
        let back = load(&out).map_err(|e| format!("reload: {e}"))?;
        Ok(Some(first_difference(&orig, &back)))
    };
    match catch_unwind(AssertUnwindSafe(run)) {
        Ok(Ok(None)) => Outcome::Same,
        Ok(Ok(Some(why))) => Outcome::Failed(why),
        Ok(Err(e)) => Outcome::Failed(e),
        Err(_) => Outcome::Failed("panic".into()),
    }
}

fn roundtrip(args: &[String], db: Option<&Index>) -> Result<(), String> {
    let dir = Path::new(&args[0]);
    let mut limit = usize::MAX;
    let mut keep = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--limit" => {
                limit = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--limit needs a number")?;
                i += 2;
            }
            "--keep" => {
                let k = PathBuf::from(args.get(i + 1).ok_or("--keep needs a directory")?);
                std::fs::create_dir_all(&k).map_err(|e| e.to_string())?;
                keep = Some(k);
                i += 2;
            }
            a => return Err(format!("unknown option {a}\n{USAGE}")),
        }
    }
    std::panic::set_hook(Box::new(|_| {}));
    let mut files = vec![];
    collect(dir, &mut files);
    let (mut same, mut failed, mut unreadable, mut not_resaved, mut raw, mut nocode) =
        (0, 0, 0, 0, 0, 0);
    let mut failures = vec![];
    let mut skipped = vec![];
    for p in &files {
        if same + failed >= limit {
            break;
        }
        let rel = p.strip_prefix(dir).unwrap_or(p).display().to_string();
        match check(p, &rel, keep.as_deref(), db) {
            Outcome::Same => same += 1,
            Outcome::Failed(why) => {
                failed += 1;
                failures.push(format!("{rel}: {why}"));
            }
            Outcome::Unreadable(why) => {
                unreadable += 1;
                skipped.push(format!("{rel}: unreal_asset cannot load it ({why})"));
            }
            Outcome::NotResaved => {
                not_resaved += 1;
                skipped.push(format!("{rel}: unreal_asset cannot re-save it unchanged"));
            }
            Outcome::RawCode => {
                raw += 1;
                skipped.push(format!("{rel}: unreal_asset cannot decode its bytecode"));
            }
            Outcome::NoCode => nocode += 1,
        }
    }
    let tested = same + failed;
    let rate = if tested > 0 {
        same as f64 * 100.0 / tested as f64
    } else {
        100.0
    };
    if db.is_some() {
        println!("reflection database:          used by asm");
    }
    println!("assets scanned:               {}", files.len());
    println!("without bytecode:             {nocode}");
    println!("not loadable by unreal_asset: {unreadable}");
    println!("not re-saved unchanged:       {not_resaved} (have bytecode)");
    println!("bytecode not decodable:       {raw}");
    println!("round-tripped:                {tested}");
    println!("byte-identical:               {same} ({rate:.2}%)");
    println!("failed:                       {failed}");
    for f in failures.iter().take(30) {
        println!("  FAIL {f}");
    }
    for s in skipped.iter().take(30) {
        println!("  SKIP {s}");
    }
    if failed > 0 {
        return Err(format!("{failed} assets did not round-trip"));
    }
    Ok(())
}
