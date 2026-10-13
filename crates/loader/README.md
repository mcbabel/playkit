# Playkit Loader

A small open source loose file loader and native mod loader for Unreal Engine 4 games on Windows and Linux (Proton).

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

## Native C++ mods

Besides loose files, the loader runs mods written in C or C++. A native mod is a DLL in `<Game>/<Project>/Binaries/Win64/mods/` that exports one function:

```cpp
#include "playkit.hpp"

PLAYKIT_MOD() {
    pk::log("hello from a C++ mod");

    // Runs before the Blueprint function, with its parameters already filled in.
    pk::hook("/Game/Interface/MatchMaking/UI_MatchMakingScreen.UI_MatchMakingScreen_C:SetScreenStyle",
             [](pk::Call& c) {
                 pk::log("PrivateMatch = %d", c.param<bool>("PrivateMatch"));
             });
}
```

Build it with MinGW (works on Linux) or MSVC:

```sh
x86_64-w64-mingw32-g++ -std=c++17 -O2 -shared -static -I crates/loader/sdk/include -o hello.dll hello.cpp
```

The headers are in [`sdk/include`](sdk/include): `playkit.h` is the plain C API, `playkit.hpp` adds C++ helpers. A complete example is in [`examples/hello`](examples/hello).

**What a mod can do:**

- **Hook Blueprint functions:** a hook runs before or after any Blueprint function, or replaces it (`call.skip = true`). This covers calls from native code, from other Blueprints and from inside the same class, because every script function passes through `ProcessLocalScriptFunction`, which the loader hooks. Hooks on classes that are not loaded yet are attached automatically as soon as they load.
- **Read and write parameters and locals by name:** `call.param<T>("Name")`.
- **Find objects by path, list objects of a class, and read fields:** `pk::field<T>(object, class, "Name")`.
- **Call any function through `ProcessEvent`.**
- **Hook native engine code:** `api->detour(target, replacement)` returns a pointer to the original. `api->scan("48 8B ?? ...")` finds code by byte pattern.

Mods are loaded on the game thread at the first Blueprint call, after the engine has started. They are loaded in file name order.

### SDK: C++ headers of the game

Create an empty file `playkit-sdk.request` next to the game executable while the game runs. A few seconds later the loader writes `playkit-sdk/` there with:

- `playkit_sdk.hpp`: every loaded class, struct and enum as C++ types at their real offsets, plus typed wrappers for every function, e.g. `sdk::AActor::K2_GetActorLocation()`. Include it instead of `playkit.hpp`.
- `reflection.json`: the same information as data (paths, sizes, property types and offsets, function flags and parameters), for tools.

Only what is loaded at that moment is included. Blueprint classes appear once the game has used them, so request the SDK after visiting the screens you want to mod.

**Engine structures:**

- The loader finds the object array and the name pool by scanning the game's data section, so it does not depend on fixed addresses.
- `ProcessLocalScriptFunction` is found by a byte pattern. If it is missing, script hooks are disabled and the log says so.

## Log

Every start writes `playkit-loader.log` next to the game executable:

```
playkit loader 0.2.0
IsNonPakFilenameAllowed at +0x29af8a0: patched
FPakFile::Find at +0x29aa970: hooked
ProcessLocalScriptFunction at +0x15ec4a0: hooked
107 loose files in playtime_multiplayer/Content
engine: GUObjectArray at +0x505cb90, FNamePool blocks at +0x5020850
engine ready: 62551 objects
[mod] hello from a C++ mod
mod hello.dll: loaded (ok)
```

If a function is reported as `not found`, the patterns do not match this game build. The game still runs normally, only that feature is off.

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
