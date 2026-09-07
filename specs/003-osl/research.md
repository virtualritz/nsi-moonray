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

## Settled

- **Texturing is OSL's, not MoonRay's.** OSL takes an OIIO
  `TextureSystem` of its own and that is what it gets. MoonRay's
  texture path -- `BasicTexture`, `UdimTexture`, `MipSelector` and the
  map DSOs over them -- exists to serve shaders written as ISPC DSOs,
  and those are exactly what OSL replaces. Sharing the two would be
  work spent making a system interoperate with its own successor.
  Decided by the author of the ɴsɪ side, not inferred here.

## Open questions

- **Displacement.** ɴsɪ has `displacementshader`; MoonRay has a
  `Displacement` root shader with the same shape as `Material`. The
  same DSO trick should apply, with `displacement()` closures.
- **Volumes.** OSL volume closures against MoonRay's `VolumeShader`.
  Later.
- **AOVs and LPEs.** MoonRay's lobe labels are how light-path
  expressions work, and OSL closures carry their own AOV names. These
  are two naming systems for the same thing and they have to be
  reconciled or one of them silently wins.
