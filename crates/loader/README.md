# Playkit Loader

A small open source loose file loader for Unreal Engine 4 games on Windows and Linux (Proton).

Shipping builds of Unreal Engine 4 ignore `.uasset`, `.uexp` and `.umap` files next to the game and only read what is inside the `.pak` archives. Playkit Loader makes the game read loose files from its `Content` folder, and those files win over the packed versions. Mods can then be installed by dropping files into the right folders, without repacking anything.

It is a `dxgi.dll` proxy. Windows loads it from the game folder before the game starts, the loader patches two engine functions in memory, and every DirectX call is passed on to the real `dxgi.dll` from the system folder. Nothing on disk is changed.

## Install

1. Download `dxgi.dll` from the latest build.
2. Put it next to the game executable, usually `<Game>/<Project>/Binaries/Win64/`.
3. On Linux or Steam Deck, add this to the game's Steam launch options:

   ```
   WINEDLLOVERRIDES="dxgi=n,b" %command%
   ```

4. Put your mod files into `<Game>/<Project>/Content/`, at the same paths they have inside the pak.

To remove it, delete `dxgi.dll`.

## What it does

| Engine function | Change |
| --- | --- |
| `FPakPlatformFile::IsNonPakFilenameAllowed` | always allows loose files, including `.uasset`, `.uexp` and `.umap` |
| `FPakFile::Find` | reports a file as missing from the pak when the same path exists in `Content`, so the engine loads the loose file |

Both functions are found by byte patterns in the running game. The list of loose files is collected once on first use; the `Paks` folder is skipped.

## Log

Every start writes `playkit-loader.log` next to the game executable:

```
playkit loader 0.1.0
IsNonPakFilenameAllowed at +0x29af8a0: patched
FPakFile::Find at +0x29aa970: hooked
107 loose files in playtime_multiplayer/Content
```

If a function is reported as `not found`, the patterns do not match this game build. The game still runs normally, only loose files are not loaded.

## Compatibility

- Tested with Unreal Engine 4.27 shipping builds made with MSVC (Project Playtime).
- Other UE 4.2x games often compile these functions the same way. The log tells you right away whether it works.
- The proxy forwards every common `dxgi.dll` export, so it can sit in front of the system DirectX or DXVK.

## Build

```sh
cargo build -p playkit-loader --release --target x86_64-pc-windows-gnu
```

On Windows, `cargo build -p playkit-loader --release` builds it with MSVC. The result is `dxgi.dll`.

## License

MIT, like the rest of [Playkit](../../README.md).
