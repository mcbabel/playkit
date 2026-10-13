use playkit_merge::{merge, Files};

fn read(path: &str) -> Files {
    let uexp = path.replace(".uasset", ".uexp");
    Files {
        uasset: std::fs::read(path).unwrap(),
        uexp: std::fs::read(uexp).unwrap(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let base = read(&args[0]);
    let mods: Vec<Files> = args[2..].iter().map(|p| read(p)).collect();
    match merge(&base, &mods) {
        Ok(out) => {
            std::fs::write(&args[1], out.uasset).unwrap();
            std::fs::write(args[1].replace(".uasset", ".uexp"), out.uexp).unwrap();
            println!("merged -> {}", args[1]);
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
