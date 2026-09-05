# Quickstart: MoonRay Backend

## Status

Spec only, but **there is a first move that does not need a heavy host**.

## Start Here

`scene_rdl2` builds without MoonRay's Embree/OpenVDB/OpenImageIO/ISPC
stack — it needs Boost, Lua, CppUnit, OpenSSL, JsonCpp, Log4cplus,
Python and TBB, all ordinary packages. See `research.md` F7.

```bash
git clone https://github.com/OpenMoonRay/scene_rdl2.git
# build per its CMakeLists; record the working invocation in this file
```

Then capture the format oracle: build a small scene through the real
library and dump it with `AsciiWriter`. **Do not infer the `.rdla`
format.** The `.nsi` emitter upstream is correct because 3Delight's own
output was read first, and it corrected four assumptions that would each
have produced a plausible, wrong file.

With that in hand, the binding-strategy question (`T0.3`) becomes an
experiment rather than an argument.

## What Still Needs A Heavy Host

Rendering. Full MoonRay is a CMake build over Embree, OpenVDB,
OpenImageIO and ISPC, normally in Docker. Scene *construction* does not
need it.

## Verification Commands

None yet.
