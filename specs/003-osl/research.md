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
silently contributes **no BSDF at all**, which renders black. And
nothing catches it: `RenderContext::canRunVectorized` checks exactly
four things -- overlapping dielectrics, volume rendering with deep
output, and reflected or refracted cryptomatte -- and never asks
whether the scene's materials have a `mShadeFuncv`.

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

Either way, **the missing check is worth reporting upstream**: a
material class that silently renders black in one execution mode and
correctly in another is the same shape of bug as `F12`.

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

## Open questions

- **Does OSL's texture path have to be MoonRay's?** Sharing
  `TextureSystem` matters for memory and for consistency with
  MoonRay's own maps; OSL will happily use its own OIIO one.
- **Displacement.** ɴsɪ has `displacementshader`; MoonRay has a
  `Displacement` root shader with the same shape as `Material`. The
  same DSO trick should apply, with `displacement()` closures.
- **Volumes.** OSL volume closures against MoonRay's `VolumeShader`.
  Later.
- **AOVs and LPEs.** MoonRay's lobe labels are how light-path
  expressions work, and OSL closures carry their own AOV names. These
  are two naming systems for the same thing and they have to be
  reconciled or one of them silently wins.
