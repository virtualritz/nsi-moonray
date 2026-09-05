# Quickstart: MoonRay Backend

## Status

`scene_rdl2` builds, and the `.rdla` format oracle is captured. Both
were done on a 4-core, 16 GB container with no renderer present.

## Building `scene_rdl2` Alone

Verified 2026-09-05 on Ubuntu 24.04, GCC 13.3, CMake 3.28.3.
`scene_rdl2` needs Boost, Lua, CppUnit, OpenSSL, JsonCpp, Log4cplus,
Python and TBB — all stock packages — plus **ISPC**, which
`research.md` F7 originally missed: `scene_rdl2/CMakeLists.txt` adds
`ISPC` to `project(... LANGUAGES ...)` and five library sources are
`.ispc`. Nothing from Embree/OpenVDB/OpenImageIO is needed; that part
of F7 holds.

```bash
sudo apt-get install -y \
    libboost-all-dev liblua5.3-dev lua5.3 libcppunit-dev libssl-dev \
    libjsoncpp-dev liblog4cplus-dev libtbb-dev python3-dev ispc

git clone --depth 1 https://github.com/OpenMoonRay/scene_rdl2.git
# The Find*.cmake modules and OMR_Platform.cmake live in a separate
# repository, and are a submodule of `openmoonray` rather than of
# `scene_rdl2`.
git clone --depth 1 https://github.com/OpenMoonRay/cmake_modules.git

# `CMAKE_MODULES_ROOT` is how MoonrayDso.cmake finds `ispc_dso_generate`
# when scene_rdl2 is built outside the `openmoonray` superbuild.
CMAKE_MODULES_ROOT=$PWD/cmake_modules cmake \
    -S scene_rdl2 -B build \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX=$PWD/install \
    -DCMAKE_MODULE_PATH=$PWD/cmake_modules/cmake
cmake --build build -j"$(nproc)"
cmake --install build
```

Roughly 15 minutes on four cores.

### Three Things That Bite

**Use the Makefile generator, not Ninja.** `lib/common/math/ispc` and
`lib/common/fb_util/ispc` set

```cmake
ISPC_HEADER_DIRECTORY /${relBinDir}
```

— a leading slash. With `-G Ninja` the generated header is declared as
an output at `/lib/common/math/ispc/…` (absolute, at the filesystem
root) while every consumer depends on `lib/common/math/ispc/…`
(relative to the build tree), and the build dies with

```
ninja: error: 'lib/common/math/ispc/Transcendental_ispc_stubs.h',
needed by 'lib/common/math/libcommon_math.so', missing and no known
rule to make it
```

Reproduced with CMake 3.23.1 — the version MoonRay's own
`install_packages.sh` downloads — as well as 3.28.3, so it is not a
CMake regression; the Makefile generator resolves the same property to
the right place and builds clean.

**One missing include.** `lib/common/grid_util/BinPacketDictionary.h`
uses `std::function` without including `<functional>`, which GCC 13
rejects:

```
BinPacketDictionary.h:77:26: error: 'function' in namespace 'std'
does not name a template type
```

Add `#include <functional>` to that header. Only `common_grid_util`
needs it; `libscene_rdl2.so` itself builds without the patch, so
`cmake --build build --target scene_rdl2` also gets you a usable
library.

### Compiling Against The Install

`scene_rdl2`'s headers do not stand on their own — its own build passes
definitions on the command line that a consumer must repeat, or
`rdl2/Types.h` fails to parse at the first `__cdecl` function typedef:

```cmake
target_compile_definitions(consumer PRIVATE
    __cdecl= PLATFORM_UNIX PLATFORM_LINUX __AVX__)
target_compile_options(consumer PRIVATE -mavx)
target_include_directories(consumer PRIVATE
    ${SCENE_RDL2_ROOT}/include ${LUA_INCLUDE_DIR})
```

`AsciiReader.h` includes `lua.hpp`, so Lua's include directory is
needed even to write a scene out.

## Capturing The Format Oracle

`tools/oracle` builds small scenes through the real library and writes
them with rdl2's own `AsciiWriter`. **Nothing about `.rdla` is
inferred** — see `oracle/` for the captured output and
`contracts/flush.md` for what it settled.

```bash
cmake -S tools/oracle -B build-oracle -DSCENE_RDL2_ROOT=/path/to/install
cmake --build build-oracle -j"$(nproc)"

# The scene classes come from rdl2's own test DSOs, which stay in the
# build tree rather than being installed.
LD_LIBRARY_PATH=/path/to/install/lib \
    ./build-oracle/oracle capture \
    /path/to/build/tests/lib/scene/rdl2 \
    specs/001-moonray-backend/oracle
```

And to check that rdl2 reads back what it wrote — and, since the
emitter produces the same bytes, what this crate writes:

```bash
LD_LIBRARY_PATH=/path/to/install/lib \
    ./build-oracle/oracle verify \
    /path/to/build/tests/lib/scene/rdl2 \
    specs/001-moonray-backend/oracle/{types,scene,blur,binding}.rdla
```

`signed_zero.rdla` is left out on purpose: rdl2's writer prints `-0`
and its reader returns `0`.

## What Still Needs A Heavy Host

Rendering. Full MoonRay is a CMake build over Embree, OpenVDB,
OpenImageIO and ISPC, normally in Docker. Scene *construction* does
not need it, and neither does checking the emitter against the oracle.

## Building The Crate

`nsi-intermediate` is overlaid from a sibling checkout, so clone it next
to this repository:

```bash
git clone https://github.com/virtualritz/nsi.git   # ../nsi
cargo test
```

A path dependency, and temporary: the crate is unpublished, and a git
dependency on that workspace makes Cargo fetch its private
`.blueprints` submodule. `[patch]` does not help — Cargo fetches the
patched source anyway, which is worth knowing before trying it. `T0.7`.

## Verification Commands

```bash
cargo test          # the emitter against the captured oracle, and the flush
```
