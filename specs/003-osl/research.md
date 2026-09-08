# Research: Running OSL Under MoonRay

ɴsɪ *is* OSL. The specification has no material model of its own: a
`shader` node names a compiled `.oso` and carries that shader's
parameters, and section 4.5 says there are no light nodes either --
geometry becomes a light when its surface shader produces an
`emission()` closure. Everything this backend currently does with
shaders (`001` `T1.3`, `T1.7a`) is a table of names standing in for a
language it cannot run.

MoonRay has no OSL (`001` `research.md` F6). This is what closing that
gap would take, read from the source at `eef67ae` rather than assumed.

## Findings

### O1: A `Material` is two functions, and only one of them is checked

`scene_rdl2::rdl2::Material` holds three function pointers, set by the
DSO in its constructor:

| | |
| --- | --- |
| `mShadeFunc` | `void(const Material*, TLState*, const State&, BsdfBuilder&)` — **scalar** |
| `mShadeFuncv` | the same over a block of SOA `Statev`, generated from ISPC |
| `mPresenceFunc` | scalar opacity, for presence/cutout |

`Material::shade` asserts `mShadeFunc` is non-null. `Material::shadev`
does **not**:

```cpp
finline void shadev(...) const
{
    if (mShadeFuncv != nullptr) {
        mShadeFuncv(this, tls, numStatev, statev, bsdfBuilderv, util::sAllOnMask);
    }
}
```

A material with no vectorised path is therefore not an error -- it
silently contributes **no BSDF at all**. And nothing catches it:
`RenderContext::canRunVectorized` checks exactly four things --
overlapping dielectrics, volume rendering with deep output, and
reflected or refracted cryptomatte -- and never asks whether the
scene's materials have a `mShadeFuncv`.

**Measured, not deduced.** `tools/scalar-material` is a `Material` DSO
with a scalar `shade` that adds one white Lambertian lobe, and
`mShadeFuncv` left null. The same scene, the same material, the two
execution modes:

| | RGB max | RGB average | alpha |
| --- | --- | --- | --- |
| `-exec_mode vector` | **0.000000** | 0.000000 | 1.0 |
| `-exec_mode scalar` | 1.018493 | 0.333256 | 1.0 |

No warning either way. The alpha channel is the sharp part: it is 1.0
in both, so the surface is present, opaque and covered -- the geometry
is found and hit, and only the shading is missing. What an author sees
is a silhouette-shaped hole, which looks like a lighting problem or a
missing assignment rather than a material that never ran.

Every material MoonRay ships is built with `moonray_ispc_dso`, so its
own shaders never take this path. `moonray_dso_simple` -- the
scalar-only DSO rule -- exists and is used for lights and geometry.

The default execution mode is `AUTO`
(`RenderOptions.cc:34`), which tries XPU, then vectorized, then
scalar. None of the four conditions apply to a scene this backend
writes, so **the default is vectorized** and a scalar-only material
would render black by default, without a warning.

That is a hard constraint on the shape of an OSL material, not a
detail: OSL's `ShadingSystem` is a scalar C++ interface (it batches
internally through LLVM, but `execute` shades one point), so an OSL
material has no ISPC `shadev` to give. The options are

1. **Force scalar.** `RenderOptions::setDesiredExecutionMode("scalar")`
   is callable from the shim, so this backend can require it whenever
   an OSL material is in the scene. Slower, correct, and the honest
   default -- and it is what any OSL renderer does at this interface.
2. **Write a C++ `ShadeFuncv`** that unpacks the SOA `Statev`, calls
   OSL per lane and packs `Bsdf`s back into `BsdfBuilderv`. The
   signature is a plain function pointer so this compiles, but the
   layouts are ISPC-generated and the pack-back is deep. Not first.

Either way, **the missing check is reported upstream**:
`upstream/moonray-scalar-material-renders-black.md`. A material class
that renders correctly in one execution mode and black in another,
with no diagnostic, is the same shape of bug as `001` `F12`.

### O2: MoonRay's self-emission is hit-only, and this is the whole light problem

`Bsdf` carries `mSelfEmission` and `BsdfBuilder::addEmission` sets it.
The integrator uses it in exactly one way, in both scalar and bundled
paths:

```cpp
Color selfEmission = pv.pathThroughput * bsdf->getSelfEmission();
radiance += selfEmission;
```

That is *all*. There is no next-event estimation toward emissive
geometry, no shadow ray drawn to it, and no importance-sampling
structure built over it. An emissive material contributes only through
paths that happen to hit it -- unbiased in the limit, and unusably
noisy for anything small and bright, which is what a light is.

Importance sampling lives on the other side of a wall, in `Light` and
its subclasses. `MeshLight` is the one that takes arbitrary geometry,
and it explicitly **refuses geometry that is in the render layer**
(`F12`), so the same surface cannot be both a sampled light and a
shaded material.

So `emission()` cannot simply become `addEmission`. In ɴsɪ that closure
*is* how lights are written -- listing 4.2's `emitter`, listing 4.3's
`spotLight` -- and mapping it to a hit-only path turns every ɴsɪ light
into noise.

### O3: A camera-visible `MeshLight` is the reconciliation

The wall has a door. `MeshLight::intersect` does not approximate: it
ray-traces the **real mesh** through an Embree scene of its own,

```cpp
rtcIntersect1(mRtcScene, &rayHit, &args);
```

and `Scene::updateActiveLights` puts every *bounded* light -- which
`MeshLight::isBounded` returns true for -- into `mVisibleLightList`
when `getIsVisibleInCamera()`. `Light::visible_in_camera` is an
enumerable Int: 0 off, 1 on, 2 (the default) deferring to
`SceneVariables::lights_visible_in_camera`.

So **one `MeshLight` with `visible_in_camera` forced on is both seen
and sampled**, against its own geometry, exactly. That is what ɴsɪ
means by an emissive mesh being ordinary geometry, and it needs no
duplicate mesh, no visibility-flag trickery and no double-counting
correction. `001` `T1.7a` sets it.

What is still lost: the mesh cannot *also* wear a non-emissive
material, because it is not in the render layer. An ɴsɪ shader that
both emits and reflects -- a glowing metal, say -- has to choose. That
is a real limitation and is reported.

### O4: The closure-to-lobe mapping is unusually good

OSL's closures and MoonRay's `BsdfComponent`s were designed by
different people for the same physics, and they line up better than
either lines up with a scene format:

| OSL closure | MoonRay component |
| --- | --- |
| `diffuse` | `LambertianBRDF` |
| `oren_nayar` | `OrenNayarBRDF` |
| `translucent` | `LambertianBTDF` |
| `reflection` | `MirrorBRDF` |
| `refraction` | `MirrorBTDF` |
| `microfacet("ggx", …)` reflect | `MicrofacetIsotropicBRDF` / `…Anisotropic…` |
| `microfacet("ggx", …)` refract | `MicrofacetIsotropicBTDF` |
| `microfacet("beckmann", …)` | the same, with `MICROFACET_DISTRIBUTION_BECKMANN` |
| `sheen` / `MaterialX sheen_bsdf` | `VelvetBRDF` |
| `subsurface` / `subsurface_bssrdf` | `RandomWalkSubsurface`, `NormalizedDiffusion`, `DipoleDiffusion` |
| `hair_*` | `HairBSDF`, `HairRBRDF`, `HairTRTBRDF`, `HairTTBTDF`, `HairTRRTBRDF` |
| `emission` | `BsdfBuilder::addEmission` — with `O2` |
| `transparent` | presence, or a mirror BTDF at IOR 1 |
| `background` | an `EnvLight`, not a material |
| `holdout` | MoonRay's cutout, through `mPresenceFunc` |
| `layer`, `mix`, `add`, scalar `*` | `BsdfBuilder`'s own weights and `BSDFBUILDER_PHYSICAL` layering |

MoonRay also has lobes OSL has no closure for -- `ToonBRDF`,
`GlitterFlakeBRDF`, `EyeCausticBRDF`, `FabricBRDF` -- which is fine:
they are unreachable from OSL and stay so.

The interesting part is `layer` and `mix`. OSL builds a closure *tree*
and the renderer flattens it; `BsdfBuilder` takes lobes in order and
does its own energy-conserving layering (`BSDFBUILDER_PHYSICAL`, as
`UsdPreviewSurface` uses). Walking OSL's tree into that order is the
real work of the mapping, and it is where a wrong answer looks
plausible.

## What this would be

Sketched, not decided:

- An `Osl` **`Material` DSO** built here, holding an
  `OSL::ShadingSystem` and a `ShaderGroup` per ɴsɪ shader network. Its
  `mShadeFunc` executes the group at the shading point and walks the
  resulting closure into a `BsdfBuilder`.
- A **`RendererServices`** implementation bridging OSL's queries --
  transforms, primitive attributes, textures, trace -- onto
  `shading::State` and MoonRay's own texture system.
- The flush stops substituting: an ɴsɪ `shader` node becomes an
  `Osl` material carrying `shaderfilename` and its parameters, and
  `PARAMETERS` (`001` F11) becomes unnecessary rather than extended.
- `LIGHTS` (`001` `T1.7a`) becomes a *fallback*: with OSL running, the
  question "does this shader emit?" is answerable by executing it and
  looking for an emission closure, rather than by recognising a name.
  That is the right answer and it is only reachable from here.

### O5: Lobe labels are a fixed vocabulary per scene class

MoonRay's material AOVs and light-path expressions both key off **lobe
labels**, and the plumbing is narrower than it first looks.

A shader passes a small integer to each `BsdfBuilder::add*` call. Zero
means "no label"; anything else indexes a `static const char *labels[]`
that the DSO declares once, at class-declaration time:

```cpp
// generated into attributes.cc by ispc_dso.py from the DSO's .json
static const char *labels[] = { "diffuse", "specular", ..., nullptr };
sceneClass.declareDataPtr("labels", labels);
```

At render prep, `RenderContext` reads that array back off the
`SceneClass` and matches each name against what the render outputs
asked for, building `lobeLabelIds` (material AOVs) and
`lpeLobeLabelIds` (light AOVs, each prefixed with the material's own
label and a dot). `aovEncodeLabels` then packs the two global ids into
one `int` -- a transformed bit, fifteen bits of material id, fifteen
of LPE id.

OSL's side is a **string**, and it is renderer-supplied: `"label"` is
not part of the language but a keyword parameter the renderer
registers on each closure. OSL's own `testrender` does exactly this --
`CLOSURE_STRING_KEYPARAM(MxDielectricParams, label, "label")` -- so a
shader writes

```
Ci = dielectric_bsdf(N, U, 1, 1, roughness, 0, ior, 0, "ggx") * w;
```

with `"label", "coat"` when it wants one.

So the reconciliation is string-to-slot, and the awkward part is that
**the slot table is per `SceneClass`, not per object**. One `Osl`
material class serves every OSL shader in the scene, and each shader
may use labels of its author's choosing.

Two ways, and the first is what the first cut should do:

1. **A fixed vocabulary.** The `Osl` class declares the conventional
   lobe names -- `diffuse`, `specular`, `transmission`, `subsurface`,
   `sheen`, `coat`, `emission`, the hair lobes -- and an OSL label
   maps to whichever slot matches. A label outside the vocabulary
   cannot be represented, and must be **reported**: an LPE naming it
   would match nothing and its AOV would render black, which looks
   like a lighting bug rather than a missing label.
2. **Fill the array late.** `declareDataPtr` stores a *pointer*, and
   `RenderContext` dereferences it at render prep -- nothing requires
   the pointed-to strings to be fixed at declaration. The DSO could
   own a mutable table and fill it with the union of labels the
   scene's OSL shaders actually use, before render prep runs. Strictly
   better, and strictly more fragile: it is global state written in
   one phase and read in another, and getting the order wrong gives
   silently wrong AOVs rather than a crash.

Either way this is the answer to "which naming system wins": neither.
OSL's strings are the input, MoonRay's integers are the output, and
the table between them is ours to declare and to report the gaps in.

### O6: rdl2 has no dynamic attributes, and OSL already solved it

An ɴsɪ `shader` node carries whatever parameters its `.oso` declares.
An rdl2 `SceneClass` declares its attributes **once**, statically, in
`RDL2_DSO_ATTR_DEFINE`. One `Osl` class serving every OSL shader in
existence therefore cannot have an attribute per parameter, and the
obvious workaround -- parallel `StringVector` / `FloatVector` /
`RgbVector` arrays with a name and type index -- is a serialization
format invented badly.

OSL ships the right one. `ShadingSystem::ShaderGroupBegin` has an
overload taking a **group specification string**:

```
param <typename> <paramname> <value>... [[hints]] ;
shader <shadername> <layername> ;
connect <layername>.<paramname> <layername>.<paramname> ;
```

That is a whole shader network -- layers, parameter values and
connections -- in one string. So the `Osl` material class needs
essentially two attributes: the group name and the group spec.
Parameters, types and topology all live in the spec, and ɴsɪ's named
shader ports become `connect` lines.

The consequence for this crate is the good one: **the flush's job
becomes a text transformation**, from an ɴsɪ shader network to an OSL
group spec. That is testable without a renderer, against strings, in
exactly the shape the `.rdla` emitter already has -- and `oslc` and
`oslinfo` are available to check that what is written parses and names
parameters the shader really has.

### O7: OSL runs here, and answers all three questions

Built and measured rather than assumed. OSL 1.13.12.0 against LLVM
18.1.3 and the system OpenImageIO 2.4.17; `tools/osl-probe` registers
three closures, builds a group from a **spec string**, executes it at
one shading point and walks the result:

```
diffuse             weight 0.2 0.7 0.9  N 0 0 1  label diffuse
microfacet(ggx)     weight 0.25 0.25 0.25  alpha 0.3  refract 0  label specular
emission            weight 2.6738 9.35831 12.0321
```

**O6 holds.** The diffuse weight is `0.2 0.7 0.9` -- the
`param color Cs 0.2 0.7 0.9` from the group spec, not the shader's own
compiled default of `0.8 0.4 0.1`. Parameters ride in the string and
override defaults, so one rdl2 `String` attribute really can carry a
whole shader network.

**The tree flattens.** `Ci` came back as an `add` of `mul`s and walked
to three lobes with weights folded in, which is the shape the
`BsdfBuilder` mapping assumes.

**O5 has an input.** `label diffuse` and `label specular` survived as
strings.

And the emission is arithmetically exact, which is the part worth
keeping: the shader is the ɴsɪ specification's own listing 4.2,
`power / (π · surfacearea) · Cs`. With `power` 42 and unit area,
`42 / π = 13.369`, times `Cs` is `2.674, 9.358, 12.032` -- what the
probe printed. **The specification's own emitter executes,
unmodified.** Which is the whole point: this is not a translation of
ɴsɪ's shading model, it *is* ɴsɪ's shading model.

Two things the build needed that no document says: `llvm-18-dev` and
`libclang-18-dev` (the runtime libraries alone are not enough), and a
`libclang-cpp.so` symlink -- Ubuntu ships only `libclang-cpp.so.18.1`,
so OSL's `FindLLVM` silently falls back to the static clang components
and the link fails on `clang::SourceMgrAdapter`.

### O8: An OSL shader renders through MoonRay

`dso/osl/` is the `Osl` material, and it works.

```
Osl("/mat") {
    ["group_spec"] = "param color Cs 0.1 0.8 0.2 ; param float roughness 0.25 ; shader red layer1 ;",
    ["search_path"] = "...",
}
```

```
$ moonray -in osl.rdla -out osl.exr -exec_mode scalar -dso_path dso/osl/build:$MOONRAY_ROOT/rdl2dso
$ oiiotool -stats osl.exr | grep 'Stats Max'
    Stats Max: 0.101849 0.814795 0.203699 1.000000 (float)
```

Exactly `Cs`, scaled by the environment light -- and changing only the
string in the `.rdla` changes the render, which is the whole chain:
rdl2 attribute, OSL group, closure tree, `BsdfBuilder`, pixels. Run
again with `0.9 0.1 0.1` it comes back `0.916644 0.101849 0.101849`,
the same ratio.

Nine closures are mapped (`diffuse`, `oren_nayar`, `translucent`,
`reflection`, `refraction`, `microfacet` both ways, `emission`), and
two are counted and reported rather than approximated: `transparent`,
which MoonRay expresses as *presence* and evaluates on its own
function before shading, and `background`, which is an environment
rather than a surface.

The one mapping decision worth stating is the **grey/coloured split**.
MoonRay's `BsdfBuilder::add*` take a *scalar* weight, and OSL closure
weights are colours. A lobe that carries no colour of its own --
`MirrorBRDF`, `MicrofacetIsotropicBRDF` in its dielectric form -- has
nowhere to put one, and folding it into a luminance renders a grey
metal. So a grey weight goes to the dielectric constructor as the
scalar weight, and a coloured one goes to the conductor constructor as
reflectivity and edge tint. That is what `UsdPreviewSurface` does with
`metallic` too, and it is exact for the case that matters.

Still standing between this and ɴsɪ: **the flush emits
`UsdPreviewSurface`**. Turning an ɴsɪ shader network into a group spec
is the remaining work, and by O6 it is a text transformation --
testable against strings, with `oslc` and `oslinfo` available to check
that what is written parses and names parameters the shader really
has.

## Settled

- **Texturing is OSL's, not MoonRay's.** OSL takes an OIIO
  `TextureSystem` of its own and that is what it gets. MoonRay's
  texture path -- `BasicTexture`, `UdimTexture`, `MipSelector` and the
  map DSOs over them -- exists to serve shaders written as ISPC DSOs,
  and those are exactly what OSL replaces. Sharing the two would be
  work spent making a system interoperate with its own successor.
  Decided by the author of the ɴsɪ side, not inferred here.

### O9: Render space follows the camera, and it hid a broken test

`RendererServices::get_matrix` is how a shader's `transform("object",
P)` is answered, and returning identity is **not an error anywhere**:
OSL asks, gets a matrix, and shades. The result is a plausible picture
of the wrong coordinate system.

MoonRay has the transforms -- `shading::Xform` gives render, world,
camera, screen and object with a `State` -- and the material now holds
one, built in `update()` as `Xform`'s own documentation asks, reached
from `RendererServices` through `ShaderGlobals::renderstate`. Both the
`State` and the `Xform` have to travel, because an instanced
prototype's object transform is per shading *point*, not per material.

The matrices are built from basis vectors -- the origin and three
axes, transformed -- rather than read out of `ispc::Xform`'s fields.
That struct exposes `mR2O` and its inverse directly, and reading them
would be right for a plain mesh and silently wrong for a crowd, since
its render-to-object entry resolves through a function pointer per
shading point. Four calls through the documented interface are exact
for every affine transform, which all of these are.

**The test for this was wrong twice, and the second failure is the one
worth recording.** It moved an object and the camera together and
compared the two renders, reasoning that object space follows the
object while render space does not. It passed with `get_matrix` stubbed
to identity.

The reason is that **MoonRay's render space follows the camera**. Move
both and render-space `P` does not change either, so the two spaces
agree and nothing can tell them apart. Translation cannot test this.

Rotation can. The quad is rotated 90° about z, so its *image footprint
is unchanged* -- only the shading can differ -- and object `(0.8, 0)`
maps to render `(0, 0.8)`, the top of the frame. A shader colouring by
`abs(P.x)` in object space is bright at the top of the quad and dark at
its centre; in render space it is dark at both. Stubbed to identity the
test now reads `0.0480` against `0.0480` and fails; with the real
matrix it passes.

The general lesson is the one this repository keeps relearning: a test
that cannot fail is worse than no test, and the only way to know is to
break the thing it tests and watch.

### O10: Three ways a group spec is wrong, and three different silences

`oslquery-petite` -- a pure-Rust `.oso` parser, no C++ -- lets the
emitted spec be checked against what the shader really declares,
before OSL sees it. Whether that is worth a dependency depends on what
OSL does with a bad spec, so it was measured with `tools/osl-probe`
rather than assumed. It does three different things:

| Spec says | OSL 1.13 does |
| --- | --- |
| a parameter the shader lacks | `WARNING: attempting to set nonexistent parameter: rooughness`, and shades on |
| a value for an **output** | accepts it, ignores it, **says nothing at all** |
| a connection from a non-output | `ERROR: ConnectShaders ...` then `ERROR: ShaderGroupBegin: error parsing group description` -- **the group does not exist** |

The first draft of this check claimed the first case was fatal. It is
not; the *third* is. Getting that backwards would have put a wrong
justification in the code, which is worse than no comment.

Each case argues for the check differently:

- The **warning** names the parameter and not the ɴsɪ node that asked
  for it, and it goes to OSL's error handler -- which this backend
  does not own and a host may have redirected. Checked here it becomes
  a limitation naming the handle.
- The **output** case is the one that earns the dependency. Nothing
  reports it anywhere: the value is accepted, discarded, and the
  shader renders its default. A scene author sees a parameter that
  does nothing.
- The **connection** case is fatal and takes the whole surface with
  it. Dropping one connection loses an input; keeping it loses the
  shader.

`Cargo.toml` carries the dependency non-optionally, because
`Shading::Osl` is a choice rather than a `cfg` -- a scene flushed on a
machine with no OSL may be rendered on one that has it, and the check
costs no C++ toolchain either way.

### O11: Displacement is a usage, not a second shading system

ɴsɪ's `displacementshader` binding and MoonRay's `Displacement` root
shader meet with less machinery than expected. OSL has **no
displacement closure**: a displacement shader assigns to `P`, and the
usage it was compiled into a group with -- `"displacement"` rather than
`"surface"` -- is what permits that. So `OslDisplacement` is `Osl` with
one string changed, and what MoonRay is handed back is `P` after minus
`P` before.

Two things were measured before writing any of it:

- MoonRay displaces a plain `RdlMeshGeometry` at `mesh_resolution` 1.
  Displacement was expected to need tessellation; it does not for a
  uniform push, and the resolution only controls how finely a *varying*
  displacement is sampled. Coverage went 0.592 → 0.800 with
  `NormalDisplacement` at height 0.5, and `OslDisplacement` running
  `P = P + 0.5 * normalize(N)` gives 0.800074 -- the same number.
- **rdl2 names are unique across classes.** `Osl("/x")` beside
  `OslDisplacement("/x")` is a hard error at scene load, not a
  shadowing. So a shader node becomes exactly one object and its
  binding decides the class.

And one thing was found only by running it: the shim's
`nmr_layer_assign` called `Layer::assign`'s four-argument overload,
which has no displacement column. Every in-process render would have
dropped the binding and rendered the undisplaced shape, silently.

### O12: OSL knows what it needs, and MoonRay needs to be told

Two of MoonRay's interfaces are pay-per-shader, and OSL's optimizer
answers both without a guess.

**Presence.** `transparent()` has no MoonRay lobe -- MoonRay expresses
straight-through transmission as *presence*, one scalar on
`mPresenceFunc`, evaluated before shading. Answering it means running
the whole network a second time, which no shader should pay for
without a `transparent()` in it. `closures_needed` is what the
optimizer found the group may emit, and `unknown_closures_needed` is
its own admission that it could not tell -- in which case the material
pays rather than rendering opaque a surface a shader asked to see
through. Measured on `amount * transparent() + (1 - amount) *
diffuse(N)`: alpha 0.592, 0.296, 0.059 at 0, 0.5, 0.9.

**Primitive attributes.** MoonRay attaches one to an intersection only
if a shader asked for it, through `Shader::mOptionalAttributes`. OSL
reports `attributes_needed`, `attribute_scopes` and `attribute_types`
for every `getattribute()` in the group, so `update()` asks, resolves
each name to a MoonRay `AttributeKey` once -- the lookup takes a lock
and `getattribute()` is the inner loop -- and requests exactly those.

Both attributes are only populated after optimization, so `update()`
calls `optimize_group` rather than waiting for the first shading point.

### O13: `uv_list` is honoured, and a test for it has to scale

An ɴsɪ mesh's `st` becomes MoonRay's `uv_list`, per face-vertex, and
reaches an OSL shader as `u` and `v`. Halving the UVs halves what the
shader reads.

**Without any `uv_list` MoonRay parametrises the face itself**, which
for a quad is also 0..1. So a test asserting that `u` varies across the
quad passes with `st` carried nowhere at all -- which is what the first
draft of that test did. It scales instead.

### O14: A shipped 3Delight shader is the test that finds things

ɴsɪ is 3Delight's interface, so `dlPrincipled.oso` -- a compiled
artefact of another renderer's shader library, built by a different
`oslc`, with no source here -- is the strongest test of this path
there is. Nothing about it can be adjusted to make it pass.

It found three bugs in a row, each hidden behind the last:

1. **A segfault inside OSL's own code generator**, before a pixel. The
   `subsurface` registration declared five formal parameters where
   `stdosl.h` declares four -- there is no `N` among them; 3Delight
   passes the normal as the keyword `"N"`. `llvm_gen_keyword_fill`
   computes where the keywords start from `nformal`, so the extra
   formal shifted every one by a slot, and OSL called `strcmp` on a
   *value* symbol it had taken for a key.

2. **Black**, because every shader 3Delight ships builds its `Ci` out
   of closures from its own `3delightosl.h`: `layer_closures`,
   `outputvariable`, `outputconstant`, `occlusion`. A renderer that
   does not register them gets an unsupported-closure error per call
   and a `Ci` with nothing in it.

3. **A white metal.** 3Delight passes a conductor's complex index of
   refraction as the `realeta` and `complexeta` keywords on
   `microfacet` rather than by tinting the closure weight -- its
   documentation says the pair "replaces the eta parameter". Dropped,
   a gold rendered `1.005, 1.005, 1.005`; carried, and routed to
   MoonRay's conductor constructor, `0.955, 0.754, 0.352`. The
   integrator's occasional NaN went with it: the artist-friendly
   constructor was being handed a weight that was never a reflectivity.

`gamma`, `thinfilmthickness`, `thinfilmeta` and `mediumeta` are not
carried. OSL warns for each by name and shades on.

### O15: OSL's `+` is a sum; MoonRay's default is a layering

`Ci = a + b` says the two closures add. `BsdfBuilder` layers by the
order lobes arrive, and `BSDFBUILDER_PHYSICAL` -- which its own header
recommends for "a typical energy-conserving material" -- makes the
first attenuate the second.

So every shader written as `diffuse() + microfacet()` lost whichever
term came second, silently, and *which* one depended on the order the
shader author happened to write. Measured on a two-lobe shader with an
AOV per lobe: one channel was black, and swapping the two terms in the
shader swapped which.

An `add` node walks its children `BSDFBUILDER_ADDITIVE` now, and the
attenuation flags are turned on only by the closures that mean
layering: MaterialX's `layer(top, base)` and 3Delight's
`layer_closures(top, bottom, mask)`. They compose, so a layer inside a
layer still layers. The two shader orderings now agree to within noise.

### O16: A lobe label reaches an AOV, and the material must stay unlabelled

The AOV question is closed, and the chain is measured end to end: an
OSL `"label"` keyword -> `label_index` against the vocabulary
`attributes.cc` declares as the scene class's `labels` -> MoonRay
reading that array at render prep -> a light-path expression naming it
-> a channel in the file. Checked by making `aov_label` return zero and
watching the channel go black.

Three things had to line up, each measured:

- **The right kind of AOV.** MoonRay's *material* AOVs name a lobe's
  properties -- `'diffuse'.albedo` is the albedo, not the
  contribution. What an output layer with `variablesource "shader"`
  asks for is the radiance, which is a *light* AOV: `C<..'diffuse'>L`.
- **The material must carry no rdl2 `label`.** `RenderContext.cc`
  registers a lobe label as `<material label>.<lobe>` when the material
  has one and as the bare lobe name when it does not. Bare is what lets
  one output layer name a lobe across every shader in the scene, which
  is what a shader AOV means.
- **`<..'x'>` wildcards both the event and the scattering type**, so
  one expression covers a lobe however it was hit.

3Delight's shaders label nothing directly -- they wrap each part of the
surface in `outputvariable("reflection", ...)` -- so the wrapper sets
the label for the closures inside it, and `reflection` becomes
`specular`, `incandescence` becomes `emission`. That table lives twice,
in `dso/osl/Osl.cc` and `src/flush.rs`, in two languages; they are one
contract and a disagreement renders the AOV black rather than failing.

### O17: `lockgeom` stays on, which is 3Delight's own position

3Delight dropped `lockgeom` outright. Its argument: symbolic linking of
geometry data to shader parameters by name is ill-defined -- what
happens when two parameters in a network share a name? -- and an
explicit connection to a node that calls `getattribute()` says the same
thing unambiguously. Any parameter with no incoming connection is
folded.

That is exactly the shape here, arrived at separately: `lockgeom 1` on
the shading system, so OSL may bake parameters that the group spec
supplies, and `RendererServices::get_attribute` answering
`getattribute()` from MoonRay's primitive attributes. The drawback
3Delight names -- no default when the primitive variable is absent --
does not apply, because `get_attribute` returning false leaves the
shader's own default in place.

### O18: `a_lobe_label_reaches_a_named_aov` crashes with SIGBUS

**Reproducible in isolation**, on OSL 1.15.6 and OpenImageIO 3.1 from
the ASWF conda channel, against MoonRay built from `main` on
2026-09-08. Every other in-process test passes, including the rest of
the OSL ones -- an arbitrary shader, a MaterialX closure, a
displacement, `st` and primitive variables, a light, the limit surface.

Two things about how it was found are worth keeping:

- **Under `cargo test` it took the whole process with it**, so fifteen
  tests that had not run yet reported nothing at all, and the failure
  read as "the in-process suite is broken". Under `cargo nextest run`
  it is one abort out of twenty-six. That reversed the reasoning behind
  the `test-rdl2` recipe, which is now nextest: what MoonRay needs is
  one renderer per process, and a process boundary supplies that rather
  than breaking it.
- The test passed in the environment it was written in. What changed is
  the OSL and OpenImageIO versions, so the first question is whether
  `O16`'s arrangement -- a lobe label reaching an AOV, with the
  material deliberately unlabelled -- still holds in OSL 1.15.

Not diagnosed. It is the one thing standing between this backend and a
green renderer suite.

## Open questions

- **An OSL volume shader.** The geometry crosses now -- the interface's
  `volume` node is OpenVDB and nothing else, and `VdbGeometry` reads
  exactly that -- but it is rendered with MoonRay's stock `VdbVolume`
  rather than the OSL network bound through `volumeshader`. Upstream
  resolves that binding and the flush now **reports** it rather than
  ignoring it, which is the honest half: a volume with no shader still
  appears, as a plausible puff of the density grid, so silence here
  reads as success.

  The interface is read off
  `scene_rdl2/lib/scene/rdl2/VolumeShader.h` rather than guessed, and
  it is worse than "four virtuals" made it sound:

  ```
  Color extinct  (tls, state, density, rayVolumeDepth)
  Color albedo   (tls, state, density, rayVolumeDepth)
  Color emission (tls, state, density)
  float anisotropy(tls, state)
  unsigned getProperties() const
  bool hasExtinctionMapBinding() const
  bool updateBakeRequired() const
  ```

  Three things follow, and each is a decision rather than a detail:

  - **Four independent questions, one execution.** Each virtual is
    asked on its own, and two of them take a `rayVolumeDepth` the
    others do not. One OSL run has to answer all four, so the closure
    tree has to be executed once per `(state, density)` and cached
    across the four calls -- keyed on the shading state, not memoised
    globally, because the volume is heterogeneous by construction.
  - **`getProperties()` is answered before anything shades.** It is a
    bitmask -- `IS_EXTINCTIVE`, `IS_SCATTERING`, `IS_EMISSIVE`, and a
    `HOMOGENOUS_*` for each -- and MoonRay uses it to decide what to
    sample at all. OSL cannot answer it without running. The only
    correct answer is the conservative superset: all three set, none
    homogeneous. That is right and slow, and saying so is better than
    a guess that silently stops sampling emission.
  - **`isHomogenous()` and the bake attributes** (`bake_resolution_mode`,
    `bake_divisions`, `bake_voxel_size`) come from the base class, so
    an `OslVolume` gets them for free and should leave them at their
    defaults until something measures otherwise.

  On OSL's side the closures are `anisotropic_vdf` and `medium_vdf`.
  The mapping is not the clean one `O4` found for surfaces: a `vdf`
  carries albedo and extinction together, and `anisotropy` is a
  parameter of the closure rather than a separate quantity, so the
  decomposition into MoonRay's four is this backend's to define.

  The DSO itself is not written. `dso/osl/OslDisplacement.cc` is the
  shape to copy -- `NSI_MOONRAY_OSL_ROOT`, `attributes.cc`,
  `shading_system.h` -- with `rdl2::VolumeShader` as the root.

  Two things measured on the way: a volume is shaded through the
  `Layer`'s **sixth** column and a row with a material in the third
  renders *nothing*, with no warning; and MoonRay's `emission_grid`
  must name an **RGB** grid -- a scalar one is refused at render prep
  and takes the whole volume with it, which matters because the
  interface's `emissiongrid` says nothing about the type.
- **`vdbparticles`.** MoonRay has no point-cloud geometry that reads an
  OpenVDB `PointDataGrid`.
- **An orthographic camera renders through `moonray` and not through
  the in-process path.** The flush emits the right class and the right
  attributes -- asserted in `flush::tests`, and the emitted `.rdla`
  hands to `moonray` and renders, in both execution modes. The same
  document applied to a live `SceneContext` and rendered
  progressively comes back empty, with no error from `apply` and no
  complaint from render prep.

  **The observation changes two things at once**, which is why it has
  not resolved: the transport (rdl2's `AsciiReader` against `apply`)
  *and* the render mode (batch against progressive). Only the second
  was named as unruled-out. Two experiments separate them, and neither
  has to render: apply the document to a live context and
  `Context::write_ascii` it, then diff against the `.rdla` that works
  -- `tests/apply.rs` is already exactly this shape -- and render that
  same working `.rdla` in process in `Mode::Batch` before
  `Mode::Progressive`.

  **The third possibility was the right one, and it is fixed.** `camera()` gives an `OrthographicCamera` no
  attributes: "orthographic and spherical cameras have no attributes of
  their own on either side". But an ɴsɪ orthographic camera has no
  `fov`, so its extent comes from the `screen` node's `screenwindow`,
  whose default the specification gives as `[-f, -1], [f, 1]` for
  `f = xres/yres` -- and **the flush carries no `screenwindow`
  anywhere**. MoonRay's orthographic frustum is
  `film_width_aperture * window`, and
  `dso/camera/OrthographicCamera/attributes.cc` defaults that to
  `24.0`. So the view is a 24-unit-wide slab of world space whatever
  the scene asked for, and a unit-sized subject lands on about one
  twenty-fourth of the frame width -- under two percent of the pixels,
  and gone entirely if it is off centre or smaller.

  Reading `ProjectiveCamera::updateImpl` settled where it goes:
  **MoonRay has no screen-window attribute at all.** `mWindow` is built
  from the aperture viewport alone, `[-1, -h/w, 1, h/w]`, every time.
  What the orthographic projection multiplies it by is
  `film_width_aperture`, so that attribute *is* the screen window's
  width in world units, and the vertical extent follows the frame.

  The two defaults agree, which is the part worth keeping: the
  interface's `[-f, -1], [f, 1]` for `f = xres/yres` is `2f` wide, and
  `2f` through MoonRay's window gives a height of exactly `2`. For a
  perspective camera the same scale is absorbed by the focal length,
  which is why this surfaced only on an orthographic one.

  A window shaped differently from the image cannot be carried and is
  reported rather than squashed. This also predicts the `.rdla` through
  `moonray` was *equally* wrong and was judged by a laxer standard --
  an image that opens, against a render asserted on pixels -- so the
  spawned render is still worth re-checking against pixels once a
  renderer is to hand.
- **AOV forwarding.** 3Delight has a per-object attribute that puts a
  diffuse surface seen in a mirror into the *diffuse* AOV rather than
  the reflection one. MoonRay's LPEs have no equivalent, and inventing
  one would mean rewriting the expression per object.
- **3Delight's hair closure.** `hair(dPdv, eta, absorption,
  sub-components)` with `hair_component("R" | "TT" | "TRT" | "TRRT",
  ...)` inside it -- a Marschner model broken into lobes. MoonRay has
  hair BSDFs of its own; whether the two decompose the same way is the
  question.
- **Scoped `getattribute`.** Answered for the unscoped form only. OSL's
  scoped form names a renderer concept ɴsɪ has no vocabulary for.
