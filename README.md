<p align="center"><img src="app/icons/128x128.png" width="96" alt=""></p>

<h1 align="center">Playkit</h1>

<p align="center">Community mod launcher for <b>Project Playtime</b>. Find mods, install them with one click, keep them up to date.</p>

> Playkit is a fan project. It is not made by or affiliated with Mob Entertainment.

## Features

- **One-click mods** from the [community mod list](https://github.com/MCbabel/playkit-mods), with updates and clean uninstall
- **Mods that change the same file work together**: Playkit merges them automatically
- **Your files stay safe**: anything a mod replaces is backed up and restored when you remove it
- **Shows what a mod means for multiplayer**: safe for public matches, or needed by everyone in a private lobby
- **Windows and Linux** (Proton, Flatpak Steam, Steam Deck) with automatic game detection
- **Own mod loader**: a small open source `dxgi.dll` that lets the game read loose files
- **No game files are uploaded**: mods are stored as binary patches against the original game files
- **Play** button starts the game through Steam as usual

## Install

Grab the latest release for your system from [Releases](https://github.com/MCbabel/playkit/releases):

| System | File |
| --- | --- |
| Windows 10/11 | `Playkit-windows-x64.exe`, no installer needed |
| Linux | `Playkit-linux-x86_64.AppImage`, make it executable and start it |

Every commit to `main` publishes a prerelease with both files and the loader `dxgi.dll` on its own.

On first start Playkit finds the game, installs the mod loader and, on Linux, adds `WINEDLLOVERRIDES="dxgi=n,b"` to the game's Steam launch options while keeping the options you already have.

## How it works

```
crates/core     game and Steam detection, mod list, install / uninstall with backups, pak access
crates/merge    combines several modded versions of the same asset into one
crates/loader   dxgi.dll proxy: forwards to the system dxgi.dll and enables loose files in UE 4.27
crates/pack     playkit-pack, the command line tool that turns a mod folder into patches
app             Tauri desktop app
ui              the app's web frontend (plain HTML, CSS and JavaScript)
```

The loader is its own small project and works with other Unreal Engine 4 games too, see [crates/loader](crates/loader/README.md). It hooks two engine functions. `FPakPlatformFile::IsNonPakFilenameAllowed` always allows loose files, and `FPakFile::Find` reports a file as missing from the pak when the same file exists in the game's `Content` folder, so the loose version wins. It writes `playkit-loader.log` next to the game executable.

## Several mods, one file

When two installed mods change the same asset, Playkit builds that file from the original game file plus every mod, in install order. It compares each mod with the original and carries over new imports, variables, functions and events, changed properties, and the hooks a mod adds to existing Blueprint code (a jump into appended code), recalculating every reference and code offset. Uninstalling a mod rebuilds the file from the mods that are left.

If two mods change the same spot in different ways, nothing is written and Playkit says which mods cannot be used together.

## Build from source

Requirements: Rust (stable), the [Tauri prerequisites](https://tauri.app/start/prerequisites/) and, on Linux, `mingw-w64` for the Windows loader.

```sh
rustup target add x86_64-pc-windows-gnu
cargo install tauri-cli --version "^2"
cargo tauri dev --config app/tauri.conf.json
cargo tauri build --config app/tauri.conf.json
```

Run the tests with `cargo test --workspace`.

## Making mods

To publish a mod, see [CONTRIBUTING in the mod list](https://github.com/MCbabel/playkit-mods/blob/main/CONTRIBUTING.md), or use **Submit a mod** inside the app.

Two tools make writing mods easier:

- **[bpscript](crates/bpscript)**: turns the Blueprint bytecode of an asset into readable text and back, byte for byte. Patch files add variables, functions and events, and hook into existing functions (`before`, `after`, `replace`). This replaces hand-written bytecode patches, and the results still combine with other mods.
- **[Native C++ mods](crates/loader#native-c-mods)**: the loader runs C and C++ mods that hook Blueprint functions, call any game function and patch native code. It can also generate typed C++ headers of the whole game.

Both can be published through Playkit. A `.bps` patch in a mod folder is applied to the original asset when the mod is packed, so it installs and combines like any other mod. A native mod ships its DLL together with its source code, which is reviewed before it is listed; Playkit marks such mods and asks before installing them.

## License

[MIT](LICENSE)
