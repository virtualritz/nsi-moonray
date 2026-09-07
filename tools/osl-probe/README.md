# Does OSL Run Here, And Answer What The Mapping Needs

Three questions, in order of how much of the design rests on them.

1. **Does a group specification string work?** `ShaderGroupBegin`'s
   text form is the answer to rdl2 having no dynamic attributes
   (`specs/003-osl/research.md` O6): one rdl2 `String` attribute
   carries a whole shader network. If it works, the flush's job is a
   text transformation and can be tested without a renderer.
2. **Does executing a shader hand back a closure tree** this can walk
   into `BsdfBuilder` calls?
3. **Do closure labels survive**, so MoonRay's material-AOV and LPE
   labels have something to map from (`research.md` O5)?

## Running it

Needs OSL built and installed:

```bash
tools/osl-probe/build.sh /path/to/osl/install
LD_LIBRARY_PATH=/path/to/osl/install/lib \
    tools/osl-probe/build/probe tools/osl-probe/build
```

`build.sh` compiles `emitter.osl` with `oslc` and links `probe.cc`
against `oslexec`. The probe registers three closures, builds a group
from a spec string, executes it at one shading point, and prints the
flattened tree.

## What it said

```
closure tree:
  diffuse             weight 0.2 0.7 0.9  N 0 0 1  label diffuse
  microfacet(ggx)  weight 0.25 0.25 0.25  alpha 0.3  refract 0  label specular
  emission            weight 2.6738 9.35831 12.0321
```

**Yes to all three.**

The diffuse weight is `0.2 0.7 0.9` — the `param color Cs 0.2 0.7 0.9`
from the group spec, not the shader's own default of `0.8 0.4 0.1`. So
parameters ride in the string and override the compiled defaults, which
is what O6 needs.

The tree came back as an `add` of `mul`s and flattened to three lobes
with their weights folded in, which is the shape the `BsdfBuilder`
mapping assumes.

`label diffuse` and `label specular` came through as strings. `"label"`
is not part of OSL — it is a keyword parameter the *renderer*
registers, which is exactly why reconciling it with MoonRay's integer
lobe labels is ours to do rather than the language's.

And the emission is arithmetically right, which is the nicest part:
the shader is the ɴsɪ specification's own listing 4.2, `power / (π ·
surfacearea) · Cs`, with `power` 42 and unit area. `42 / π = 13.369`,
times `Cs` gives `2.674, 9.358, 12.032` — what the probe printed. The
specification's emitter executes, unmodified.

## Environment

OSL 1.13.12.0, built here against LLVM 18.1.3 and the system
OpenImageIO 2.4.17. Two things the build needed that were not obvious:
`llvm-18-dev` and `libclang-18-dev` (the runtime libraries alone are
not enough), and a `libclang-cpp.so` symlink — Ubuntu ships only
`libclang-cpp.so.18.1`, so OSL's `FindLLVM` falls back to the static
clang components and the link fails on `clang::SourceMgrAdapter`.
