use playkit_merge::{load, Files};
use unreal_asset::exports::ExportBaseTrait;
use unreal_asset::types::fname::{FName, FNameContainer};

fn read(path: &str) -> Files {
    Files {
        uasset: std::fs::read(path).unwrap(),
        uexp: std::fs::read(path.replace(".uasset", ".uexp")).unwrap(),
    }
}

fn norm<T: Clone + FNameContainer>(v: &T) -> T {
    let mut c = v.clone();
    c.traverse_fnames(&mut |n: &mut FName| *n = FName::new_dummy(n.get_content(), n.get_number()));
    c
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = load(&read(&args[0])).unwrap();
    let b = load(&read(&args[1])).unwrap();
    let names = |x: &playkit_merge::Asset| {
        x.get_name_map()
            .get_ref()
            .get_name_map_index_list()
            .to_vec()
    };
    let (na, nb) = (names(&a), names(&b));
    println!(
        "names {} vs {} (same order: {})",
        na.len(),
        nb.len(),
        na == nb
    );
    let imports_same = a.imports.len() == b.imports.len()
        && a.imports
            .iter()
            .zip(&b.imports)
            .all(|(x, y)| norm(x) == norm(y));
    println!(
        "imports {} vs {} same: {}",
        a.imports.len(),
        b.imports.len(),
        imports_same
    );
    let (ea, eb) = (&a.asset_data.exports, &b.asset_data.exports);
    println!("exports {} vs {}", ea.len(), eb.len());
    let mut diff = 0;
    for (i, (x, y)) in ea.iter().zip(eb).enumerate() {
        let strip = |e: &unreal_asset::exports::Export| {
            let mut e = norm(e);
            let base = e.get_base_export_mut();
            base.serial_offset = 0;
            base.serial_size = 0;
            e
        };
        if strip(x) != strip(y) {
            diff += 1;
            if diff <= 8 {
                let (dx, dy) = (format!("{:?}", strip(x)), format!("{:?}", strip(y)));
                let p = dx
                    .bytes()
                    .zip(dy.bytes())
                    .position(|(c, d)| c != d)
                    .unwrap_or(0);
                let s = p.saturating_sub(120);
                println!(
                    "export {i} {} differs:\n  A: …{}…\n  B: …{}…",
                    x.get_base_export().object_name.get_content(),
                    &dx[s..(p + 200).min(dx.len())],
                    &dy[s..(p + 200).min(dy.len())]
                );
            }
        }
    }
    println!("differing exports: {diff}");
}
