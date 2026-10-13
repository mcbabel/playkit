# unreal_asset (vendored)

Copy of [unreal_asset 0.1.16](https://github.com/AstroTechies/unrealmodding) (MIT) with these changes:

- `FunctionExport` reads and writes the `RepOffset` of network functions
- `ExMapConst` writes the element count instead of the number of keys and values
- public fields on `FScriptText`, `FEnumProperty` and `FBoolProperty`
- length check for `UnknownProperty`
- `kismet::walk`: generic traversal of every sub-expression and package index
- removed the Ace Combat 7 decryption and the Oodle link setup
