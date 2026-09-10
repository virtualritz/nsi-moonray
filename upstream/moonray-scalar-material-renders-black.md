<!--
Ready to file at https://github.com/OpenMoonRay/moonray/issues/new

Title: A Material or Map without a vectorized entry point is silently
       skipped in vectorized mode

Not filed from here: this session's GitHub access is scoped to
`virtualritz`, and the MoonRay repository is on another tier.

Everything below was read from the source at `eef67ae` and reproduced
by running it -- see `specs/003-osl/research.md` O1, and
`tools/scalar-material/` for the probe.
-->

# A `Material` or `Map` with no vectorized entry point is skipped, silently

## Summary

`rdl2::Material` carries a scalar `mShadeFunc` and a vectorized
`mShadeFuncv`. `Material::shade` asserts the first is set;
`Material::shadev` null-checks the second and does nothing when it is
absent. So a `Material` DSO that implements only scalar shading is not
rejected -- it contributes **no BSDF at all** in vectorized mode, and
the surface renders black.

Nothing warns. `RenderContext::canRunVectorized` -- the function whose
job is to decide whether the scene can run vectorized -- checks four
unrelated things and never asks whether the scene's materials have a
vectorized path. The default execution mode is `AUTO`, which selects
vectorized when those four checks pass, so **the default is the mode
that produces the wrong image**.

## Where

`scene_rdl2/lib/scene/rdl2/Material.h:50-58`:

```cpp
finline void shadev(moonray::shading::TLState *tls,
                    unsigned numStatev,
                    const rdl2::Statev* const statev,
                    rdl2::BsdfBuilderv *bsdfBuilderv) const
{
    if (mShadeFuncv != nullptr) {
        mShadeFuncv(this, tls, numStatev, statev, bsdfBuilderv, util::sAllOnMask);
    }
}
```

versus the scalar one immediately above it:

```cpp
MNRY_ASSERT(mShadeFunc != nullptr);
```

`moonray/lib/rendering/rndr/RenderContext.cc:3406` (`canRunVectorized`)
fails on, in its own order: overlapping dielectrics, volume rendering
with deep output, reflected cryptomatte, refracted cryptomatte. That is
the whole list.

`moonray/lib/rendering/rndr/RenderOptions.cc:34` sets the default mode
to `AUTO`.

## Reproducing

A `Material` DSO whose constructor sets `mShadeFunc` and leaves
`mShadeFuncv` null, adding one Lambertian lobe:

```cpp
ScalarProbe::ScalarProbe(const SceneClass& sceneClass, const std::string& name)
    : Parent(sceneClass, name)
{
    mShadeFunc  = ScalarProbe::shade;
    mShadeFuncv = nullptr;
}

void
ScalarProbe::shade(const rdl2::Material* self, shading::TLState*,
                   const State& state, BsdfBuilder& bsdfBuilder)
{
    const LambertianBRDF lambert(state.getN(), static_cast<const ScalarProbe*>(self)->get(attrColor));
    bsdfBuilder.addLambertianBRDF(lambert, 1.0f, ispc::BSDFBUILDER_PHYSICAL, 0);
}
```

assigned to a quad lit by an `EnvLight`, rendered both ways:

```
$ moonray -in probe.rdla -out probe.vector.exr -exec_mode vector
$ moonray -in probe.rdla -out probe.scalar.exr -exec_mode scalar
$ oiiotool -stats probe.vector.exr | grep 'Stats Max'
    Stats Max: 0.000000 0.000000 0.000000 1.000000 (float)
$ oiiotool -stats probe.scalar.exr | grep 'Stats Max'
    Stats Max: 1.018493 1.018493 1.018493 1.000000 (float)
```

Neither run prints a warning.

Note the **alpha**: 1.0 in both. The geometry is found, hit and opaque;
only the shading is missing. What the author sees is a
silhouette-shaped hole, which reads as a lighting problem or a bad
layer assignment rather than as a material that never ran.

## Why this is not hypothetical

Every material shipped in `moonray/dso/material` is built with
`moonray_ispc_dso`, so the code path is never exercised in-tree. But
`moonray_dso_simple` exists and is used for lights and geometry, and
nothing marks it as unusable for materials.

More to the point, **any shading language embedded in MoonRay lands
here**. OSL's `ShadingSystem` shades one point at a time; so does
MaterialX's reference implementation, and so does any interpreter. A
material class that calls into one has no ISPC function to offer, and
this is the first thing it hits.

## The same hole in `Map`, and it is worse

`rdl2::Map` has the same pair -- `mSampleFunc` and `mSampleFuncv` --
and the same asymmetry. A scalar-only `Map` bound to a `MeshLight`'s
`map_shader` is not rejected in vectorized mode; the light simply
falls back to its own `color` times `intensity`.

That is worse than the material case, because the fallback is not
black. It is a **plausible image of the wrong light**. A mesh light
whose radiance is an OSL closure -- a tinted screen, a gradient, a
texture -- renders as a flat white lamp in the right place at roughly
the right brightness, and nothing about the frame says a shader was
skipped.

Reproduced with the same scene twice, differing only in the mode. The
map returns the shader's `tint` of `(1, 0.25, 0.02)`:

```
$ moonray -in emitter.rdla -exec_mode scalar -out scalar.exr
$ moonray -in emitter.rdla -exec_mode vectorized -out vector.exr
$ oiiotool --stats scalar.exr | grep 'Stats Max'
    Stats Max: 707.222107 176.805527 14.144444 1.000000 (float)
$ oiiotool --stats vector.exr | grep 'Stats Max'
    Stats Max: 23.540888 23.540888 23.540888 1.000000 (float)
```

The scalar run is the tint exactly: `707 : 177 : 14` is `1 : 0.25 :
0.02`. The vectorized run is neutral grey, and does not move when the
shader's parameters are changed -- editing `tint` to pure green or
raising `gain` tenfold leaves the image identical to five decimal
places, which is how this was identified at all.

`MeshLight::eval` reaches the map through `sampleMapShader`, which
calls `map->sample(...)` -- the scalar entry point -- so the C++ path
is correct. The vectorized integrator takes another route and finds
nothing to call.

Whatever `canRunVectorized` grows for materials should cover the maps
reachable from a light in the same walk.

## Suggested fixes, in order of preference

1. **Have `canRunVectorized` ask.** Walk the layer's materials and
   fail with `"material <name> has no vectorized shade function"`. It
   already walks them for the dielectric-priority check, so the loop
   exists. `AUTO` then falls back to scalar and the image is right.
2. **Warn once at render prep** if a material with no `mShadeFuncv`
   is reached in vectorized mode. Cheap, and turns a silent wrong
   image into a line an author can act on.
3. **Assert, as the scalar path does.** Consistent, but it removes
   the possibility of a scalar-only material rather than accommodating
   one -- which is the wrong direction if MoonRay ever wants an
   embedded shading language.

(1) and (2) are complementary; either alone is a large improvement on
a black image and no message.

## Workaround here

`nsi-moonray` forces `RenderOptions::setDesiredExecutionMode("scalar")`
whenever the build carries OSL at all -- materials and light maps
alike, since both are the same shading system -- and reports that it
did so. The render is slower and correct rather than fast and wrong.
