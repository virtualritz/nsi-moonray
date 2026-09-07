# `Osl` — an OSL surface as a MoonRay material

ɴsɪ *is* OSL. A `shader` node names a compiled `.oso` and carries that
shader's parameters, and section 4.5 of the specification says a light
is geometry whose surface shader produces an `emission()` closure. So
this is not a translation of ɴsɪ's shading model — running it *is* the
mapping, and `PARAMETERS` and `LIGHTS` in `flush.rs` become
unnecessary rather than extended.

## Building

```bash
dso/osl/build.sh [moonray-install] [osl-install]
```

Compiled directly rather than through MoonRay's `moonray_dso_simple`,
which lives in MoonRay's build tree and whose CMake config pulls in a
CppUnit that nothing here needs. Produces `Osl.so` and
`Osl.so.proxy`; put the directory on MoonRay's DSO path.

Needs OSL built. On Ubuntu that means `llvm-18-dev` and
`libclang-18-dev` — the runtime libraries alone are not enough — and a
`libclang-cpp.so` symlink, since Ubuntu ships only
`libclang-cpp.so.18.1` and OSL's `FindLLVM` otherwise falls back to
the static clang components and fails to link.

## Two strings, not a parameter list

rdl2 declares a class's attributes once, statically. One class serving
every OSL shader in existence therefore cannot have an attribute per
parameter, and parallel name/type/value arrays would be a
serialization format invented badly.

OSL already has the right one. `ShaderGroupBegin` takes a **group
specification** — layers, parameter values and connections, as text:

```
param color Cs 0.1 0.8 0.2 ;
param float roughness 0.25 ;
shader red layer1 ;
connect layer1.Cout layer2.Cs ;
```

So the whole ɴsɪ shader network arrives in one `String` attribute, and
the flush's job is a text transformation — testable without a
renderer, in the shape the `.rdla` emitter already has.

## Scalar only, and it matters

`mShadeFuncv` is left null. OSL's `ShadingSystem` shades one point at
a time and has no ISPC function to offer, and MoonRay's
`Material::shadev` null-checks the pointer and silently does nothing —
so **in vectorized mode, which is the default, this renders black with
no diagnostic**. Measured in `tools/scalar-material`, reported as
`upstream/moonray-scalar-material-renders-black.md`.

Whoever builds the render has to force scalar execution. A scene
handed to `moonray` by hand needs `-exec_mode scalar`.

## The closure mapping

OSL hands back a tree of `add`, `mul` and component nodes and leaves
the flattening to the renderer; `BsdfBuilder` takes lobes in order and
does its own energy-conserving layering. Weights fold down the tree,
which is what makes the two compatible at all.

| OSL closure | MoonRay |
| --- | --- |
| `diffuse` | `LambertianBRDF`, colour as albedo |
| `oren_nayar` | `OrenNayarBRDF` |
| `translucent` | `LambertianBTDF`, normal reversed |
| `reflection` | `MirrorBRDF` — dielectric when the weight is grey, conductor when it is not |
| `refraction` | `MirrorBTDF`, colour as tint |
| `microfacet(…, refract 0)` | `MicrofacetIsotropicBRDF`, same grey/coloured split |
| `microfacet(…, refract 1)` | `MicrofacetIsotropicBTDF` |
| `emission` | `BsdfBuilder::addEmission` |
| `transparent`, `background` | counted and reported — see below |

The grey/coloured split is not a shortcut. MoonRay's `add*` methods
take a *scalar* weight, so a coloured weight has nowhere to go on a
lobe that has no colour of its own; folding it into a luminance would
render a grey metal. A coloured specular is a conductor, and MoonRay's
artist-friendly constructor takes reflectivity and edge tint — which
is how `UsdPreviewSurface` spells metal too.

`transparent` is straight-through transmission, which MoonRay
expresses as *presence* — evaluated by its own function before
shading, so a closure has nowhere to land. `background` is an
environment, and reaches MoonRay as an `EnvLight` from the ɴsɪ
`environment` node rather than through a material.

## Emission is not a light

`emission()` becomes `addEmission`, and that is **hit-only**: MoonRay's
integrator does `radiance += pathThroughput * bsdf->getSelfEmission()`
and nothing else — no next-event estimation, no shadow rays. An ɴsɪ
emitter meant to *light* the scene becomes a `MeshLight` in the flush
instead, forced visible in camera so it is seen as well as sampled.
`specs/003-osl/research.md` O2 and O3.

## Labels

MoonRay's material AOVs and LPEs key off an integer per lobe, indexing
a static array declared on the scene class. OSL's side is a string —
`"label"` is a keyword parameter the *renderer* registers, not part of
the language. `attributes.cc` declares the vocabulary; a label outside
it leaves the lobe unnamed rather than renaming it, because an LPE
naming a label that never registered renders black.

## What it said

A quad with an OSL shader, rendered by MoonRay:

```
group_spec = "param color Cs 0.1 0.8 0.2 ; param float roughness 0.25 ; shader red layer1 ;"
→ Stats Max: 0.101849 0.814795 0.203699
```

Exactly `Cs`, scaled by the environment light. Changing only the
string in the `.rdla` changes the render, which is the whole chain:
rdl2 attribute → OSL group → closures → `BsdfBuilder` → pixels.
