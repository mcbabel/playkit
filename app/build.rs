use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let candidates = [
        std::env::var("PLAYKIT_LOADER_DLL").ok().map(PathBuf::from),
        Some(root.join("target/x86_64-pc-windows-gnu/release/dxgi.dll")),
        Some(root.join("target/x86_64-pc-windows-msvc/release/dxgi.dll")),
        Some(root.join("target/release/dxgi.dll")),
    ];
    let dll = candidates
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .expect("build the loader first: cargo build -p playkit-loader --release --target x86_64-pc-windows-gnu");
    println!("cargo:rerun-if-changed={}", dll.display());
    println!("cargo:rerun-if-env-changed=PLAYKIT_LOADER_DLL");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("dxgi.dll");
    std::fs::copy(&dll, out).expect("copy loader");
    tauri_build::build();
}
