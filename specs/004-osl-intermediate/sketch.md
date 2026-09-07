# `osl-intermediate` — A Sketch

A draft for someone else to start from. Written from having just
integrated OSL into a MoonRay backend by hand
(`specs/003-osl/research.md`, `dso/osl/`, `src/osl.rs`), so the shape
below is what that work kept wanting and not having.

Nothing here is built. It is a proposal with the reasoning attached,
because the reasoning is the part worth arguing with.

## What it is for

OSL is how a renderer stops inventing a shading language. Three things
stand between a Rust renderer and using it, and none of them is OSL
itself:

1. **Reading `.oso` without linking OSL.** A scene converter, a
   validator, a DCC exporter or an asset pipeline needs to know a
   shader's parameters, types and defaults. Today that means linking
   `liboslquery`, which drags in LLVM.
2. **Describing a shader network.** OSL's own group-specification
   string is the only portable form, and every renderer re-derives how
   to build one. Getting it wrong is quiet: an integral float that
   should have been `1.0` is refused as an int, a layer name with a
   dot makes a `connect` ambiguous, a parameter that never arrives
   renders the shader's default.
3. **Mapping closures onto a renderer's BSDF model.** The tree is
   `add`/`mul`/component and the flattening is the same everywhere;
   the lobe vocabulary is not.

(1) and (2) need no OSL at all. (3) does, and should be a separate
crate.

## Shape

Four crates, so a consumer takes only what it needs. Names are
placeholders.

```
osl-oso        parse .oso            no deps beyond std
osl-group      build a group spec    depends on osl-oso (optional)
osl-closure    closure tree → lobes  needs liboslexec
osl-sys        raw bindings          needs liboslexec
```

`osl-intermediate` is then a facade re-exporting `osl-oso` and
`osl-group`, in the way `nsi-intermediate` is one thing with layers
inside it. Someone who wants to *run* shaders adds `osl-closure`.

**The split that matters is `osl-group` not needing LLVM.** Building a
shader network is what an exporter, a converter and a scene format all
do, and none of them wants a compiler.

## `osl-oso` — reading a compiled shader

`.oso` is a **text** format. The header is one line, then metadata,
then `param`/`oparam` declarations, then code. The declarations are
all a consumer needs:

```
surface dlPrincipled %meta{string,niceName,"Principled"}
param	color	i_color	0.800000012 0
param	float	roughness	0.300000012 0
oparam	closure color	outColor	 %read{...} %write{...}
```

The current backend already greps these (`tools/probe/parameters.sh`)
and it is enough to build a parameter table. A real parser gives:

```rust
pub struct Shader {
    pub kind: ShaderKind,          // surface, displacement, volume, shader
    pub name: String,
    pub metadata: Vec<Metadata>,
    pub parameters: Vec<Parameter>,
}

pub struct Parameter {
    pub name: String,
    pub kind: TypeDesc,            // float, color, point, matrix, string, arrays
    pub output: bool,
    pub default: Option<Value>,    // absent when computed rather than literal
    pub metadata: Vec<Metadata>,   // `label`, `page`, `widget`, `min`, `max`
    pub connectable: bool,
}
```

**Why this earns its place:** a group spec that names a parameter the
shader does not have fails at `ShaderGroupBegin`, and the message
names the parameter but not who asked for it. With the declarations in
hand, `osl-group` can refuse — or report — *before* OSL is involved,
which is the difference between a diagnostic at export time and a
render that does not start.

Metadata matters more than it looks: `page`, `label`, `widget`, `min`
and `max` are what a UI needs, and reading them is why `oslinfo`
exists. A Rust crate that answers the same questions without a C++
toolchain is immediately useful to tools that have no renderer at all.

**Version tolerance.** `.oso` carries `OpenShadingLanguage 1.00` on its
first line and has been stable for a decade, but a parser should keep
unknown lines rather than reject them, and expose the raw text. The
alternative — a crate that stops working on the next OSL release — is
worse than no crate.

## `osl-group` — describing a network

The output is OSL's own group specification:

```text
param <typename> <paramname> <value>... [[hints]] ;
shader <shadername> <layername> ;
connect <layername>.<paramname> <layername>.<paramname> ;
```

The API should be a builder that cannot express an invalid group:

```rust
let mut group = Group::new("nsi_shader");
let texture = group.layer("dlTexture", "tex")?
    .set("texturename", "/tex/wall.tx")?
    .set("scale", 2.0)?;
let surface = group.layer("dlPrincipled", "surface")?
    .set("roughness", 0.25)?;
group.connect(texture, "outColor", surface, "i_color")?;
let spec: String = group.finish();
```

With `osl-oso` available, `set` and `connect` check the parameter
exists and the types are compatible; without it they take the caller's
word and only check syntax. Same API, two strengths — which is the
"no lack of utility without" property.

**Four things a correct emitter has to do**, every one of which the
MoonRay backend got wrong first and fixed against OSL's own parser:

- **An integral float keeps its decimal point.** `param float gain 1`
  is read as an int and refused. `1.0` works. This is a one-character
  bug that only shows up on round numbers.
- **Layers must be declared before they are connected**, and a
  layer's `param` lines must precede its `shader` line — OSL applies
  parameters to the *next* shader. So a network is emitted deepest
  first.
- **Layer names cannot contain `.`**, because that separates a layer
  from a parameter in `connect`. Scene handles routinely do (they are
  paths), so a name mapping is not optional.
- **Strings need escaping**, and so does the surrounding format. The
  MoonRay backend emits the whole spec on one line because it travels
  as a Lua string in a scene file and a raw newline is a syntax error
  there. A crate should offer both — `to_string()` and
  `to_string_single_line()` — rather than pick.

**Hints** (`[[ ... ]]`) carry `lockgeom`, `interpolated` and the like
and should be expressible; they are how a renderer says a parameter is
constant and may be baked in.

## `osl-closure` — the tree, and the flattening

This one needs `liboslexec`, so it is separate.

What every renderer writes by hand:

```rust
pub enum Closure<'a> {
    Add(&'a Closure<'a>, &'a Closure<'a>),
    Mul(Color, &'a Closure<'a>),
    Component { id: u32, weight: Color, params: &'a [u8] },
}

/// Fold the weights down the tree and hand back a flat list.
pub fn flatten(root: &Closure, into: &mut Vec<Lobe>);
```

The flattening is identical in every renderer — weights multiply down
`mul`, both sides of `add` are visited — and it is worth writing once.
What is *not* shared is the lobe vocabulary, so `flatten` should hand
back `(id, weight, &[u8])` and let the renderer decode its own
registered parameter structs.

The other half is **closure registration**, which is a pile of
`repr(C)` structs and offset tables that must agree field-for-field
with what OSL writes. A crate can ship the standard closures'
declarations — `diffuse`, `oren_nayar`, `translucent`, `reflection`,
`refraction`, `microfacet`, `transparent`, `emission`, `background`,
`subsurface`, the hair family, and the MaterialX set — as safe Rust
types with a derive that generates the `ClosureParam` table. Getting
one offset wrong reads garbage, silently, so this is exactly the kind
of thing to write once and test once.

**`"label"` is not part of OSL.** It is a keyword parameter the
*renderer* registers, which is how AOV and light-path names reach a
closure. A crate should register it on every closure by default,
because a renderer that forgets cannot name its lobes and will not
find out until someone writes an LPE.

## How it legos with `nsi-intermediate`

ɴsɪ *is* OSL: a `shader` node names a compiled `.oso` and carries that
shader's parameters, and a named shader port is a connection between
two of them. So the join is small and belongs in neither crate:

```rust
// in nsi-moonray, or an `nsi-osl` bridge crate
pub fn group(scene: &nsi_intermediate::Scene, root: &str) -> osl_group::Group
```

`src/osl.rs` in this repository is that function, at about 200 lines
including its type mapping, and roughly half of it is the four
correctness points above — which is exactly the half that should move
into `osl-group`.

**Neither crate should depend on the other.** `nsi-intermediate` has
no business knowing about OSL's serialization, and `osl-group` has no
business knowing what an ɴsɪ handle is. The bridge is a function in
whoever needs both. That is the same relationship `nsi-moonray` has
with `scene_rdl2`, and it is what has kept both ends replaceable.

The one thing worth *adding* to `nsi-intermediate` is an accessor that
answers "what is the shader network rooted at this handle?" — the
reachable set through `EdgeKind::ShaderNetwork`, in dependency order.
That is graph knowledge, it belongs upstream, and every backend
wanting OSL will otherwise write the same traversal.

## What to do first

In order, because each one is useful alone:

1. **`osl-oso`.** No dependencies, a text format, and immediately
   useful to anything that wants to know a shader's parameters. It is
   also the easiest to test: OSL ships 178 compiled shaders in the
   free 3Delight download, and `oslinfo` is the oracle — parse each
   one and diff against `oslinfo`'s output. That is a real conformance
   suite for an afternoon's work.
2. **`osl-group`**, with `osl-oso` optional. Test it against OSL's own
   parser through a tiny C shim, the way `src/osl.rs`'s
   `what_this_emits_is_what_osl_parses` does — a test asserting what
   the emitter was *expected* to write cannot catch a spec that is
   well-formed to its author and refused by the parser. That test
   failed on its first run here and was worth every line.
3. **`osl-closure`**, once something needs to run shaders.

## Things to decide, not assumed here

- **Whether `osl-sys` should exist separately** or each crate binds
  what it needs. OSL's C++ API has no C wrapper, so bindings mean a
  hand-written `extern "C"` shim regardless — `dso/osl/` in this
  repository is one, at about 200 lines for the shading half.
- **Whether to vendor or find OSL.** A `build.rs` that locates an
  install is friendlier than vendoring something that needs LLVM. On
  Ubuntu the non-obvious requirements are `llvm-N-dev` *and*
  `libclang-N-dev`, plus a `libclang-cpp.so` symlink — Ubuntu ships
  only the versioned one and OSL's `FindLLVM` silently falls back to
  static clang and fails to link.
- **MaterialX closures.** OSL 1.13+ ships `mx_*` closures and shaders,
  and they are the direction the industry is going. Registering them
  is more surface but it is the surface people will want.
- **Whether `osl-group` should also *parse* a group spec.** Round-trip
  would make it testable against itself, and OSL's own serialization
  produces one — but nothing needs it yet, and a parser is a
  commitment.
