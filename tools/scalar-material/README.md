# A Material With No Vectorised Shade

MoonRay's `Material` holds two shading functions, a scalar one and a
vectorised one generated from ISPC. `Material::shadev` null-checks the
second:

```cpp
if (mShadeFuncv != nullptr) {
    mShadeFuncv(this, tls, numStatev, statev, bsdfBuilderv, util::sAllOnMask);
}
```

OSL has no ISPC path to give — its `ShadingSystem` shades one point at
a time — so an OSL material is exactly this shape. What MoonRay does
with it had to be measured before anything was built on top.

`ScalarProbe` is that material and nothing more: one flat Lambertian
lobe from a scalar `shade`, `mShadeFuncv` deliberately null. A black
render can only mean the material was not run.

## Running it

```bash
tools/scalar-material/build.sh /path/to/install
```

`build.sh` compiles the two translation units directly rather than
through `moonray_dso_simple`. That rule is the right one for a shipped
DSO and it is what a real OSL material would use — but it lives in
MoonRay's build tree, and `SceneRdl2Config.cmake` pulls in a CppUnit
this container does not have, so a probe that needs neither should not
need both. The flags are the ones `build.rs` uses for the shim, for
the same reason: rdl2's headers assume them.

Then, with a scene assigning `ScalarProbe` to a lit quad:

```bash
moonray -in probe.rdla -out probe.exr -exec_mode vector \
        -dso_path tools/scalar-material/build:$MOONRAY_ROOT/rdl2dso
```

## What it said

| | RGB max | RGB average | alpha |
| --- | --- | --- | --- |
| `-exec_mode vector` | **0.000000** | 0.000000 | 1.0 |
| `-exec_mode scalar` | 1.018493 | 0.333256 | 1.0 |

No warning either way, and the default execution mode is `AUTO`, which
picks vectorized.

The alpha channel is the sharp part. It is 1.0 in both, so the surface
is present, opaque and covered — the geometry is found and hit, and
only the shading is missing. What an author sees is a
silhouette-shaped hole, which reads as a lighting problem or a missing
assignment rather than as a material that never ran.

`specs/003-osl/research.md` O1 for what this constrains;
`upstream/moonray-scalar-material-renders-black.md` for the report.
