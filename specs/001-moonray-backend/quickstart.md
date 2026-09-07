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

## Building MoonRay Itself

Only needed to *render*. Nothing else here does — the emitter, the
oracle and the flush are all checked without it.

**Status: verified.** This recipe built MoonRay on the container
described above — about 50 minutes on four cores — and the renderer it
produced rendered a scene this crate flushed. Each of the five problems
named below stopped the build until it was worked around.

One caveat: `MOONRAY_BUILD_TESTING=NO` does not stop the test binaries
being configured, and two of them fail to link. `cmake --build
build-moonray --target moonray` builds the renderer without them.

The dependencies are all in Ubuntu 24.04 except OpenSubdiv and
OpenImageDenoise:

```bash
sudo apt-get install -y \
    libembree-dev libopenvdb-dev libopenimageio-dev openimageio-tools \
    libopenexr-dev libimath-dev librandom123-dev libjpeg-dev \
    zlib1g-dev libblosc-dev bison flex libcurl4-openssl-dev \
    libmicrohttpd-dev
```

Then, with `scene_rdl2` already built and installed into `$PREFIX`:

```bash
git clone --depth 1 --branch v3_5_0 \
    https://github.com/PixarAnimationStudios/OpenSubdiv.git
cmake -S OpenSubdiv -B build-osd -DCMAKE_INSTALL_PREFIX=$PREFIX \
    -DCMAKE_BUILD_TYPE=Release -DNO_TBB=1 -DNO_PTEX=1 -DNO_OPENGL=1 \
    -DNO_CUDA=1 -DNO_OPENCL=1 -DNO_DX=1 -DNO_METAL=1 -DNO_OMP=1 \
    -DNO_TESTS=1 -DNO_GLTESTS=1 -DNO_EXAMPLES=1 -DNO_TUTORIALS=1 \
    -DNO_REGRESSION=1 -DNO_DOC=1 -DBUILD_SHARED_LIBS=ON
cmake --build build-osd -j"$(nproc)" && cmake --install build-osd

curl -sSLO https://github.com/OpenImageDenoise/oidn/releases/download/v2.3.0/oidn-2.3.0.x86_64.linux.tar.gz
tar xzf oidn-2.3.0.x86_64.linux.tar.gz
cp -a oidn-2.3.0.x86_64.linux/include/* $PREFIX/include/
cp -a oidn-2.3.0.x86_64.linux/lib/*     $PREFIX/lib/

git clone --depth 1 https://github.com/OpenMoonRay/mcrt_denoise.git
export PATH=$PREFIX/bin:$PATH   # moonray's DSO build runs rdl2_json_exporter
CMAKE_MODULES_ROOT=$PWD/cmake_modules cmake -S mcrt_denoise -B build-denoise \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=$PREFIX \
    -DCMAKE_PREFIX_PATH=$PREFIX -DCMAKE_MODULE_PATH=$PWD/cmake_modules/cmake \
    -DMOONRAY_USE_OPTIX=NO
cmake --build build-denoise -j"$(nproc)" && cmake --install build-denoise

CMAKE_MODULES_ROOT=$PWD/cmake_modules cmake -S moonray -B build-moonray \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=$PREFIX \
    -DCMAKE_PREFIX_PATH=$PREFIX -DCMAKE_MODULE_PATH=$PWD/cmake_modules/cmake \
    -DMOONRAY_USE_OPTIX=NO -DMOONRAY_BUILD_TESTING=NO \
    -DOpenSubDiv_INCLUDE_DIR=$PREFIX/include/opensubdiv \
    -DOpenSubDiv_CPU_LIBRARY=$PREFIX/lib/libosdCPU.so \
    -DOpenSubDiv_GPU_LIBRARY=$PREFIX/lib/libosdCPU.so
cmake --build build-moonray -j"$(nproc)" && cmake --install build-moonray
```

### Five Things That Bite

Each of these stops the build outright, and none of them is a code
problem:

1. **OpenSubdiv 3.5's TBB evaluator does not compile against oneTBB.**
   `osd/tbbEvaluator.cpp` includes `tbb/task_scheduler_init.h`, removed
   in oneTBB 2021. Build with `-DNO_TBB=1`; MoonRay uses the CPU
   `Far`/`Vtr` side.
2. **`MoonRay's FindOpenSubDiv` requires an `osdGPU`** that a CPU-only
   OpenSubdiv does not build, and `find_package_handle_standard_args`
   fails on the `NOTFOUND`. Pointing `OpenSubDiv_GPU_LIBRARY` at
   `libosdCPU.so` gets past it; nothing links GPU subdivision.
3. **OpenImageDenoise cannot be built from a plain clone** — its
   trained weights are Git LFS pointers and the build refuses them —
   and `mcrt_denoise` needs **2.x**, not 1.4: it uses
   `OIDN_DEVICE_TYPE_CUDA`, which 1.4 does not have. The release
   tarball ships weights, libraries and a CMake config.
4. **`MOONRAY_USE_OPTIX=NO`, or CUDA is required.** Both `moonray` and
   `mcrt_denoise` `find_package(CUDAToolkit REQUIRED)` otherwise and
   die on a missing `nvcc`.
5. **Ubuntu's OpenImageIO CMake config references files it does not
   install**: `/usr/bin/iconvert`, which is in `openimageio-tools`, and
   `/usr/include/opencv4`, which comes from OpenCV. Installing the
   tools package fixes the first; the second is satisfied by the
   directory merely existing.

### And Two Paths, Not One

MoonRay's DSO build shells out to `rdl2_json_exporter`, which
`scene_rdl2` installed into `$PREFIX/bin`. Putting that on `PATH` is
not enough -- the loader then cannot resolve the library it needs, and
the build dies partway through the DSOs with

```
rdl2_json_exporter: error while loading shared libraries:
libscene_rdl2.so: cannot open shared object file
```

`$PREFIX/lib` is on no default search path, so **both** are needed:

```bash
export PATH=$PREFIX/bin:$PATH
export LD_LIBRARY_PATH=$PREFIX/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
```

Worth knowing that this one appears *late*: everything configures, the
libraries build, and the first DSO to need a JSON export is where it
stops.

## Building The Shim

The `extern "C"` surface over `scene_rdl2` that lets a scene be built
in memory rather than written to a file. `build.rs` drives it for the
crate; this is how it is built and checked on its own:

```bash
cmake -S shim -B build-shim -DSCENE_RDL2_ROOT=$PREFIX
cmake --build build-shim -j"$(nproc)"

# Drives every setter through rdl2's own `ExtensiveObject`, which
# declares an attribute of every type, and writes the result out.
./build-shim/shim_smoke \
    /path/to/build-rdl2/tests/lib/scene/rdl2 out.rdla
```

The smoke test is not decoration. It is what proved the calls work
against the real library rather than merely compiling against its
headers -- including that a missing attribute and a mistyped one come
back as *different* codes, which is what a useful limitation report
depends on.

## Building The Authoring Twins

Declarations of MoonRay's DSOs with no implementation, so a mesh scene
can be built and read back with `scene_rdl2` alone -- no renderer.

```bash
cmake -S tools/twin -B build-twin \
    -DSCENE_RDL2_ROOT=$PREFIX -DMOONRAY_SOURCE=/path/to/moonray
cmake --build build-twin -j"$(nproc)"

export NSI_MOONRAY_DSO=$PWD/build-twin   # for `tests/apply.rs`
```

`MOONRAY_SOURCE` is a *source checkout*, not an install: each twin
compiles MoonRay's own `attributes.cc` verbatim, so it cannot drift
from what the renderer declares. A clone is enough; nothing is built
from it.

`UsdPreviewSurface` has no twin. Its `attributes.cc` is generated from
an `.ispc` by MoonRay's own build, so building one would need exactly
what these avoid needing.

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

**Build the library before testing.** `tests/dropin.rs` `dlopen`s
`target/debug/libnsi_moonray.so` by path, so Cargo does not know the
test depends on it and can run against the *previous* one. A stale
artefact never looks like a stale artefact -- it looks like a bug you
have already fixed (`002` `research.md` F7).


```bash
cargo test          # the emitter against the captured oracle, and the flush

# With the renderer linked. `--lib` first, per the note above.
export SCENE_RDL2_ROOT=$PREFIX MOONRAY_ROOT=$PREFIX
export NSI_MOONRAY_DSO=$PREFIX/rdl2dso
cargo build --features rdl2 --lib
cargo test --features rdl2
```

`$NSI_MOONRAY_SCENE` writes the `.rdla` for whatever renders,
in-process or spawned. It is a dump rather than a transport, and it is
how you get at the scene a render was actually made from.
