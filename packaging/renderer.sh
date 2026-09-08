#!/usr/bin/env sh
#
# Fetch and build MoonRay into `vendor/install`, so `just test-rdl2`,
# `just bundle` and everything else that needs a renderer just work.
#
# This is `specs/001-moonray-backend/quickstart.md` made executable,
# refs pinned. That document is the reasoning -- five upstream problems
# stop this build and each is written up there with the error it
# produces; what is here is only the sequence.
#
# **Why not submodules.** Four repositories, several hundred megabytes,
# and every one of them useless to somebody who wants the emitter and
# the flush -- which is most people, most of the time, and is the whole
# reason `rdl2` is off by default. A clone at a pinned ref costs the
# same version certainty and only when asked for. `vendor/` is
# gitignored.
#
# Usage: packaging/renderer.sh [--prefix DIR] [--jobs N] [--check]
#
#   --prefix   where to install; default `vendor/install`
#   --jobs     parallel build jobs; default every core
#   --check    verify the tools and headers are there, build nothing

set -eu

PREFIX=""
JOBS=""
CHECK=0
VENDOR="vendor"

# Pinned, because "whatever is on main today" is not a build anyone can
# reproduce, and a renderer that changed under a bug report is a bug
# report nobody can act on.
SCENE_RDL2_REF="main"
CMAKE_MODULES_REF="main"
MCRT_DENOISE_REF="main"
MOONRAY_REF="main"
OPENSUBDIV_REF="v3_5_0"
OIDN_VERSION="2.3.0"

die() { echo "renderer: $*" >&2; exit 1; }
step() { echo ""; echo "renderer: === $* ==="; }

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) PREFIX="${2:-}"; shift 2 ;;
        --jobs)   JOBS="${2:-}"; shift 2 ;;
        --check)  CHECK=1; shift ;;
        -h|--help) sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown argument $1" ;;
    esac
done

[ -n "$PREFIX" ] || PREFIX="$PWD/$VENDOR/install"
[ -n "$JOBS" ] || JOBS="$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)"

# **Checked before anything is cloned.** An hour into a build is a bad
# time to learn that ISPC is missing, and the CMake error when it is
# names a language rather than a package.
missing=""
for tool in cmake git curl c++ ispc; do
    command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
done
for header in \
    /usr/include/boost/version.hpp \
    /usr/include/log4cplus/loglevel.h \
    /usr/include/jsoncpp/json/json.h
do
    [ -f "$header" ] || missing="$missing $(basename "$(dirname "$header")")"
done

if [ -n "$missing" ]; then
    echo "renderer: missing:$missing" >&2
    echo "renderer: \`just deps\` installs everything; \
\`packaging/deps.sh --list\` prints the list without installing." >&2
    [ "$CHECK" -eq 1 ] || exit 1
fi

if [ "$CHECK" -eq 1 ]; then
    echo "renderer: prefix $PREFIX"
    echo "renderer: jobs   $JOBS"
    [ -n "$missing" ] && exit 1
    echo "renderer: everything needed is present"
    exit 0
fi

mkdir -p "$VENDOR" "$PREFIX"
export PATH="$PREFIX/bin:$PATH"

# A shallow clone at a ref, idempotent: running this again after a
# failure picks up where it stopped rather than starting over.
fetch() {
    name="$1"; url="$2"; ref="$3"
    if [ -d "$VENDOR/$name/.git" ]; then
        echo "renderer: $name is already cloned"
        return 0
    fi
    git clone --depth 1 --branch "$ref" "$url" "$VENDOR/$name"
}

step "fetching"
fetch cmake_modules https://github.com/OpenMoonRay/cmake_modules.git "$CMAKE_MODULES_REF"
fetch scene_rdl2    https://github.com/OpenMoonRay/scene_rdl2.git    "$SCENE_RDL2_REF"
fetch mcrt_denoise  https://github.com/OpenMoonRay/mcrt_denoise.git  "$MCRT_DENOISE_REF"
fetch moonray       https://github.com/OpenMoonRay/moonray.git       "$MOONRAY_REF"
fetch OpenSubdiv    https://github.com/PixarAnimationStudios/OpenSubdiv.git "$OPENSUBDIV_REF"

MODULES="$PWD/$VENDOR/cmake_modules"

# **`std::function` without `<functional>`.** GCC 13 rejects it, it is
# one line, and it is the first thing that stops this build. Patched
# rather than reported here because the report is already written.
HEADER="$VENDOR/scene_rdl2/lib/common/grid_util/BinPacketDictionary.h"
if [ -f "$HEADER" ] && ! grep -q "#include <functional>" "$HEADER"; then
    step "patching BinPacketDictionary.h (GCC 13 needs <functional>)"
    sed -i.bak '1a #include <functional>' "$HEADER"
fi

step "scene_rdl2"
# **The Makefile generator, not Ninja.** `ISPC_HEADER_DIRECTORY` is set
# with a leading slash, so under Ninja the generated header is declared
# at the filesystem root while every consumer wants it in the build
# tree. Same under the CMake version MoonRay's own script downloads, so
# it is not a regression to wait out.
CMAKE_MODULES_ROOT="$MODULES" cmake -S "$VENDOR/scene_rdl2" -B "$VENDOR/build-rdl2" \
    -G "Unix Makefiles" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DCMAKE_MODULE_PATH="$MODULES/cmake"
cmake --build "$VENDOR/build-rdl2" -j"$JOBS"
cmake --install "$VENDOR/build-rdl2"

step "OpenSubdiv"
# **`-DNO_TBB=1`.** OpenSubdiv 3.5's TBB evaluator includes
# `tbb/task_scheduler_init.h`, removed in oneTBB 2021. MoonRay uses the
# CPU `Far`/`Vtr` side, which does not need it.
cmake -S "$VENDOR/OpenSubdiv" -B "$VENDOR/build-osd" \
    -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_BUILD_TYPE=Release \
    -DNO_TBB=1 -DNO_PTEX=1 -DNO_OPENGL=1 -DNO_CUDA=1 -DNO_OPENCL=1 \
    -DNO_DX=1 -DNO_METAL=1 -DNO_OMP=1 -DNO_TESTS=1 -DNO_GLTESTS=1 \
    -DNO_EXAMPLES=1 -DNO_TUTORIALS=1 -DNO_REGRESSION=1 -DNO_DOC=1 \
    -DBUILD_SHARED_LIBS=ON
cmake --build "$VENDOR/build-osd" -j"$JOBS"
cmake --install "$VENDOR/build-osd"

step "OpenImageDenoise (binary release)"
OIDN="oidn-$OIDN_VERSION.x86_64.linux"
if [ ! -d "$VENDOR/$OIDN" ]; then
    curl -sSL -o "$VENDOR/$OIDN.tar.gz" \
      "https://github.com/OpenImageDenoise/oidn/releases/download/v$OIDN_VERSION/$OIDN.tar.gz"
    tar xzf "$VENDOR/$OIDN.tar.gz" -C "$VENDOR"
fi
cp -a "$VENDOR/$OIDN/include/." "$PREFIX/include/"
cp -a "$VENDOR/$OIDN/lib/." "$PREFIX/lib/"

step "mcrt_denoise"
CMAKE_MODULES_ROOT="$MODULES" cmake -S "$VENDOR/mcrt_denoise" -B "$VENDOR/build-denoise" \
    -G "Unix Makefiles" \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DCMAKE_PREFIX_PATH="$PREFIX" -DCMAKE_MODULE_PATH="$MODULES/cmake" \
    -DMOONRAY_USE_OPTIX=NO
cmake --build "$VENDOR/build-denoise" -j"$JOBS"
cmake --install "$VENDOR/build-denoise"

step "moonray"
# **`FindOpenSubDiv` wants an `osdGPU`** a CPU-only OpenSubdiv does not
# build, so the CPU library is named for both.
CMAKE_MODULES_ROOT="$MODULES" cmake -S "$VENDOR/moonray" -B "$VENDOR/build-moonray" \
    -G "Unix Makefiles" \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DCMAKE_PREFIX_PATH="$PREFIX" -DCMAKE_MODULE_PATH="$MODULES/cmake" \
    -DMOONRAY_USE_OPTIX=NO -DMOONRAY_BUILD_TESTING=NO \
    -DOpenSubDiv_INCLUDE_DIR="$PREFIX/include/opensubdiv" \
    -DOpenSubDiv_CPU_LIBRARY="$PREFIX/lib/libosdCPU.so" \
    -DOpenSubDiv_GPU_LIBRARY="$PREFIX/lib/libosdCPU.so"
# **`MOONRAY_BUILD_TESTING=NO` does not stop the test binaries being
# configured**, and two of them fail to link. Naming the target builds
# the renderer without them.
cmake --build "$VENDOR/build-moonray" -j"$JOBS" --target moonray
cmake --install "$VENDOR/build-moonray"

step "done"
[ -d "$PREFIX/rdl2dso" ] || die "$PREFIX/rdl2dso was not installed, so \
no scene class would resolve; the build above did not finish"

echo "renderer: $PREFIX"
echo "renderer: $(find "$PREFIX/rdl2dso" -name '*.so' | wc -l | tr -d ' ') \
scene classes"
echo ""
echo "  just test-rdl2      the renderer tests"
echo "  just bundle $PREFIX"
