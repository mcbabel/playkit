# bpscript

Read and edit Blueprint bytecode (Kismet) of Unreal Engine 4.27 assets as text.

- `dis` prints every function of a `.uasset`/`.umap` as readable code.
- `asm` compiles that text back. Text that was not edited gives the identical
  bytes (`.uasset` and `.uexp`).
- `patch` applies a mod file: new variables, functions, events and hooks on top
  of an existing asset. Original instructions keep their offsets, so patched
  assets stay combinable with `playkit-merge`.

## Commands

```
bpscript dis <asset.uasset|.umap> [out.bps]
bpscript asm <asset> <in.bps> <out.uasset>
bpscript patch <in.uasset> <mod.bps> <out.uasset>
bpscript roundtrip <dir> [--limit N] [--keep DIR]
```

`asm` and `patch` read the `.uexp` next to the input and write both files.
`asm` replaces the bytecode of every function that appears in the text;
functions left out keep their code. `roundtrip` runs dis and asm on every asset
with bytecode below `<dir>` and reports how many are byte-identical. Assets that
unreal_asset itself cannot load or re-save unchanged are counted separately.
`--keep DIR` stores the disassembly of every asset.

## Text syntax

```
function RefreshPlayerInviteButtons {
    push L1534;
    Count = 0;
    CallFunc_GetMultiplayerGameInstance_ReturnValue = Default__PlaytimeFunctionLibrary_C.GetMultiplayerGameInstance(self);
    K2Node_SwitchEnum_CmpSuccess = NotEqual_ByteByte(CallFunc_IsSteamIDValid_Exec_Result, 0u8);
    if !K2Node_SwitchEnum_CmpSuccess goto L216;
    pop;
L216:
    PrintString(self, "NO VALID LOBBY", true, true, LinearColor{1.0, 0.068442, 0.0, 1.0}, 10.0);
    pop;
L1534:
    return;
}

function Construct {
    ExecuteUbergraph_UI_MatchMakingScreen(@L2873);
    return;
}
```

One statement per line, ending with `;`. Labels (`L1534:`) replace byte offsets;
generated labels carry the original offset in the name. All offsets (jumps,
pushes, latent resume points, context skip sizes, switch offsets) are computed
by the assembler. The final `EndOfScript` is implicit.

### Statements

| Text | Kismet |
| --- | --- |
| `a = b;` | `Let`, `LetBool`, `LetObj`, `LetDelegate`, ... picked from the variable type |
| `goto L;` / `goto *Expr;` | `Jump` / `ComputedJump` |
| `if !Cond goto L;` | `JumpIfNot` |
| `push L;` `pop;` `pop if !Cond;` | execution flow stack |
| `return;` / `return Expr;` | `Return` |
| `Del += Fn;` `Del -= Fn;` `clear Del;` | multicast delegates |
| `Arr = [1, 2];` `S = set[1];` `M = map[1: 2];` | `SetArray`, `SetSet`, `SetMap` |
| `nop;` `end;` | `Nothing`, `EndOfScript` |

### Expressions

| Text | Meaning |
| --- | --- |
| `Name` | local, parameter, class variable, or an object of the asset |
| `obj.Var`, `obj.Func(...)` | context; `obj.Member` on a struct is a struct member |
| `Func(...)` | call; final, math (static native) or virtual is inferred |
| `self`, `none`, `true`, `false` | constants |
| `5`, `5u8`, `5i64`, `5u64`, `1.5` | int, byte, int64, uint64, float |
| `"text"`, `n"Name"` | string, name |
| `vec(x, y, z)`, `rot(p, y, r)`, `Margin{1.0, 2.0, 3.0, 4.0}` | vector, rotator, struct |
| `nsloctext(ns, key, src)`, `invtext(s)`, `text(s)`, `loctable(t, id, key)`, `emptytext` | text |
| `cast<Class>(e)`, `primcast<ObjectToBool>(e)` | casts |
| `arr[i]` | array element by reference |
| `switch(i) {0 => a, 1 => b, _ => c}` | select |
| `bind(Del, Obj, FuncName)`, `broadcast(Del, args...)` | delegates |
| `@L123` | address of a label (latent resume point); inside a call of `ExecuteUbergraph_*` the entry offset into the ubergraph |

### References

Short names work when they are unique in the asset: `PrintString`,
`KismetSystemLibrary::PrintString`. Full paths use Unreal's notation in single
quotes, optionally with the class: `'/Script/Engine.KismetSystemLibrary:PrintString'`,
`WidgetBlueprintGeneratedClass'/Game/Interface/MainMenu/UI_MainMenu.UI_MainMenu_C'`.
A variable of another class is `Owner::Name`. Names that are not identifiers
are written in backticks: `` `FP Player Mesh Container` ``.

A full path that is not in the asset yet becomes a new import. The class is
inferred where possible (function calls, structs, `/Script` classes, functions
inside a class, `Default__` objects); otherwise write it, like
`BlueprintGeneratedClass'/Game/Base/MultiplayerGameInstance.MultiplayerGameInstance_C'`.
`import Ref;` at the top level creates an import up front so that its short name
can be used afterwards.

### Annotations

The default notation is what the Kismet compiler would emit. When the original
bytecode differs, `dis` writes the difference explicitly and `asm` keeps it:

| Annotation | Forces |
| --- | --- |
| `local X`, `out X`, `inst X`, `default X`, `sparse X` | variable kind |
| `math F()`, `final F()`, `localfinal F()`, `virtual F()`, `localvirtual F()` | call token |
| `let x = ...`, `letbool`, `letobj`, `letweak`, `letdelegate`, `letmulticast`, `letframe` | assignment token |
| `let(Owner::Prop) x = ...` | property of a `Let` (`let(none)` for none) |
| `a?.b`, `a@.b`, `a->b` | fail-silent context, class context, struct member |
| `a.[rv=Owner::Prop, skip=12]b` | context r-value property or skip size |
| `Margin<16>{...}` | struct size |
| `a"..."`, `u"..."` | ANSI or UTF-16 string |
| `obj(Ref)` | object constant where a variable has the same name |
| `Foo#3` | raw name number |
| `#-12` | raw package index |

Inference uses the asset itself: declared properties, and how each property,
function and struct is already used in the bytecode. Well under 1% of the
statements in the game's assets need an annotation.

## Patch files

```
import '/Script/Engine.KismetSystemLibrary:PrintString';

var Counter: int;                                    // class variable
local ExecuteUbergraph_X::Lobby: struct<SteamID, 8>;  // local of an existing function

function Add(A: int, B: const ref struct<Vector>, C: out int) -> bool {
    local Tmp: int;
    C = A;
    ReturnValue = true;
}

event Clicked(Data: const ref struct<JoinLobbyData, 16>, bOk: bool) {
    if !bOk goto done;
    Counter = 1;
done:
}

before Construct {
    PrintString(self, "construct", true, true, LinearColor{1.0, 0.0, 0.0, 1.0}, 2.0);
}

after Tick ubergraph { Counter = 2; }

replace SillyShit { return; }
```

- Types: `bool`, `byte`, `int`, `int64`, `float`, `string`, `name`, `text`,
  `object<Class>`, `class<Class>`, `struct<Struct>` or `struct<Struct, size>`,
  `delegate<Signature>`, `multicast<Signature>`, `array<T>`. Parameters can be
  `const ref`, `ref` or `out`. Common engine struct sizes are known; other
  structs need the size.
- `function` creates a new function with its own code; `-> T` adds `ReturnValue`.
- `event` creates a Blueprint event: a stub function that copies the parameters
  into ubergraph locals named `<Event>_<Param>` and calls the event code, which
  is appended to the ubergraph. Inside the event the parameters keep their
  names. `return;` ends the event. Events and functions can be bound with
  `bind(...)` and `+=`.
- `before F { }` runs at the start of `F`: the first statement becomes a jump to
  appended code, which runs the hook, the original statement and jumps back.
- `after F { }` runs before every `return` of `F`. A return that is too small for
  a jump is reached through retargeted jumps and pushes (and, when the code
  falls through into it, through the statement before it).
- `replace F { }` replaces the whole body.
- `ubergraph` after the function name (`before Construct ubergraph { }`) places
  the hook code in the ubergraph, where ubergraph locals and latent actions are
  available; the hooked function then calls it.
- Inside hooks the function's own locals and parameters are visible by name;
  `local Name: type;` in a body adds a local to the function the code runs in.
- Labels in ubergraph code (events and ubergraph hooks) share one namespace, so
  events can jump into code of another event.

`examples/invite.bps` is a port of uatool `pminvite` (Steam friend invites in the
private match lobby), `examples/invite_gi.bps` of `pminvitegi`.

### Testing invite.bps in the game

1. `cargo build --release -p bpscript`
2. `toolchain/uatool/target/release/uatool pmscreen2 pakex/orig/Playtime_Multiplayer/Content/Interface/MatchMaking/UI_MatchMakingScreen.uasset <work>/pm2.uasset`
3. `bpscript patch <work>/pm2.uasset crates/bpscript/examples/invite.bps <work>/UI_MatchMakingScreen.uasset`
4. `bpscript patch pakex/orig/Playtime_Multiplayer/Content/Base/MultiplayerGameInstance.uasset crates/bpscript/examples/invite_gi.bps <work>/MultiplayerGameInstance.uasset`
   (or keep the GameInstance from the current mod set if it already has `PM_InviteCmdDone`).
5. Back up the installed loose files, then install both outputs (`.uasset` and
   `.uexp`) in place of the `pminvite`/`pminvitegi` results.
6. Private match lobby: the invite row appears top left; the free slot shows "+".
   Clicking it opens the Steam overlay invite dialog (without overlay the Steam
   friends list). A friend accepting the invite while the game runs, or starting
   the game through the invite (`+connect_lobby`), joins the lobby from the main
   menu. Same test as for uatool: `lobby_fake_host_start` and
   `steam steam://joinlobby/1961460/<lobby>/<steamid>`.
7. The mod is local only (no RPC, no replicated variable); `uatool netsig` of the
   output is identical to the uatool result.

Differences to the uatool result (same new functions, locals and imports):
calls to `SetVisibility` are virtual instead of final, contexts on `GetParent()`
have no r-value property, the parameter copies are called `PM_InviteRequest_Data`
instead of `MP_PM_InviteRequest_Data`, uatool's debug-only imports and the unused
`BooleanAND` import are missing.

## Limits

- UE 4.27 cooked assets (field path properties, 32 bit float constants).
- Assets that unreal_asset cannot load or re-save unchanged are not supported
  (a few widgets with MovieScene data in this game).
- Types of imported functions and properties are unknown. Unused imported
  functions are inferred as math calls without a context and final calls with
  one; unknown members need `Owner::Name` unless the left side has a known type
  (declared variable, `self`, `cast<...>`).
- `before` needs a first statement of at least 5 bytes, `after` a return that is
  reachable through a jump or a large enough statement before it. A function
  that jumps back to its first statement runs a `before` hook again.
- Patched functions get new imports only for the code; preload dependencies of
  exports are copied from a template event and not recomputed.
- New struct sizes are not checked against the engine.
