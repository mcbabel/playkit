use playkit_reflect::Db;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage:
  playkit-sdk gen <reflection.json> [playkit_sdk.hpp]   generate the C++ header
  playkit-sdk merge <out.json> <in.json>...             merge databases from several game sessions
  playkit-sdk stats <reflection.json>                   count classes, structs, enums and functions";

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("gen") if args.len() >= 2 => {
            let db = Db::load(&PathBuf::from(&args[1]))?;
            let out = args
                .get(2)
                .map_or(PathBuf::from("playkit_sdk.hpp"), PathBuf::from);
            std::fs::write(&out, playkit_reflect::generate(&db))
                .map_err(|e| format!("{}: {e}", out.display()))?;
            println!("{}", out.display());
            Ok(())
        }
        Some("merge") if args.len() >= 3 => {
            let mut db = Db::default();
            for p in &args[2..] {
                db.merge(Db::load(&PathBuf::from(p))?);
            }
            db.save(&PathBuf::from(&args[1]))?;
            println!(
                "{} classes and structs, {} enums -> {}",
                db.classes.len(),
                db.enums.len(),
                args[1]
            );
            Ok(())
        }
        Some("stats") if args.len() >= 2 => {
            let db = Db::load(&PathBuf::from(&args[1]))?;
            let classes = db.classes.iter().filter(|c| c.kind == "class").count();
            let functions: usize = db.classes.iter().map(|c| c.functions.len()).sum();
            let game = db
                .classes
                .iter()
                .filter(|c| c.path.starts_with("/Game/"))
                .count();
            println!(
                "{classes} classes ({game} from /Game), {} structs, {} enums, {functions} functions",
                db.classes.len() - classes,
                db.enums.len()
            );
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
