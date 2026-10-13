use playkit_merge::{contains, Files};

fn read(path: &str) -> Files {
    Files {
        uasset: std::fs::read(path).unwrap(),
        uexp: std::fs::read(path.replace(".uasset", ".uexp").replace(".umap", ".uexp")).unwrap(),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    println!("{:?}", contains(&read(&a[0]), &read(&a[1]), &read(&a[2])));
}
