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

# **Installed where an installed `mnry` looks**, which is the whole
# point: `src/dso.rs` searches the platform's per-user data directory,
# so a renderer put here is found by a binary from
# `cargo install --path .` with no flag and no environment. Building
# into the checkout instead would need `--dso-path` forever.
#
# Sources and build trees stay in `vendor/`; only the install escapes.
if [ -z "$PREFIX" ]; then
    case "$(uname -s)" in
        Darwin) PREFIX="$HOME/Library/Application Support/MoonRay" ;;
        *)      PREFIX="${XDG_DATA_HOME:-$HOME/.local/share}/moonray" ;;
    esac
fi
[ -n "$JOBS" ] || JOBS="$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)"

# OpenImageDenoise ships a build per platform and architecture, and the
# shared-library suffix follows the same split.
case "$(uname -s)/$(uname -m)" in
    Darwin/arm64)  OIDN_PLATFORM="arm64.macos";  SHLIB="dylib" ;;
    Darwin/*)      OIDN_PLATFORM="x86_64.macos"; SHLIB="dylib" ;;
    *)             OIDN_PLATFORM="x86_64.linux"; SHLIB="so" ;;
esac

# **The pixi environment, when the checkout has one.** It is the same
# set of dependencies on Linux and macOS, resolved from one lockfile,
# and it needs no root -- which is why it is preferred over the system
# packages `packaging/deps.sh` installs. Everything below looks there
# first and falls back to the system.
PIXI_ENV="$PWD/.pixi/envs/default"
if [ -d "$PIXI_ENV" ]; then
    export PATH="$PIXI_ENV/bin:$PATH"
    export CMAKE_PREFIX_PATH="$PIXI_ENV${CMAKE_PREFIX_PATH:+:$CMAKE_PREFIX_PATH}"
    SEARCH="$PIXI_ENV/include"
else
    PIXI_ENV=""
    SEARCH="/usr/include /usr/local/include /opt/homebrew/include"
fi

have_header() {
    for root in $SEARCH; do
        [ -f "$root/$1" ] && return 0
    done
    return 1
}

# **Checked before anything is cloned.** An hour into a build is a bad
# time to learn that ISPC is missing, and the CMake error when it is
# names a language rather than a package.
missing=""
for tool in cmake git curl c++ ispc; do
    command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
done
for header in boost/version.hpp json/json.h; do
    have_header "$header" || missing="$missing $header"
done

# `log4cplus` is the one gap in the pixi route, and only on macOS:
# conda-forge builds it for linux-64 and win-64 and not for osx-arm64.
# Built from source below rather than sending someone to a second
# package manager for one library.
NEED_LOG4CPLUS=0
if ! have_header log4cplus/loglevel.h; then
    if [ -n "$PIXI_ENV" ]; then
        NEED_LOG4CPLUS=1
    else
        missing="$missing log4cplus/loglevel.h"
    fi
fi

if [ -n "$missing" ]; then
    echo "renderer: missing:$missing" >&2
    echo "renderer: \`just pixi-install\` gets all of it without root, \
on both platforms. \`just deps\` uses system packages instead." >&2
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

if [ "$NEED_LOG4CPLUS" -eq 1 ] && [ "$CHECK" -eq 0 ]; then
    step "log4cplus (no osx-arm64 conda build; from source)"
    mkdir -p "$VENDOR"
    if [ ! -d "$VENDOR/log4cplus/.git" ]; then
        git clone --depth 1 --branch REL_2_1_2 --recurse-submodules \
            https://github.com/log4cplus/log4cplus.git "$VENDOR/log4cplus"
    fi
    cmake -S "$VENDOR/log4cplus" -B "$VENDOR/build-log4cplus" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$PIXI_ENV" \
        -DLOG4CPLUS_BUILD_TESTING=OFF \
        -DWITH_UNIT_TESTS=OFF \
        -DBUILD_SHARED_LIBS=ON
    cmake --build "$VENDOR/build-log4cplus" -j"$JOBS"
    cmake --install "$VENDOR/build-log4cplus"
fi

step "fetching"
fetch cmake_modules https://github.com/OpenMoonRay/cmake_modules.git "$CMAKE_MODULES_REF"
fetch scene_rdl2    https://github.com/OpenMoonRay/scene_rdl2.git    "$SCENE_RDL2_REF"
fetch mcrt_denoise  https://github.com/OpenMoonRay/mcrt_denoise.git  "$MCRT_DENOISE_REF"
fetch moonray       https://github.com/OpenMoonRay/moonray.git       "$MOONRAY_REF"

MODULES="$PWD/$VENDOR/cmake_modules"

# **Two headers used without being included.** Both are one line, and
# both stop the build outright. Which of them you hit depends on the
# compiler: GCC 13 rejects the first, clang 18 rejects both, and a
# standard library that happens to include them transitively rejects
# neither -- which is why they reached a release at all.
patch_include() {
    file="$VENDOR/scene_rdl2/$1"
    header="$2"
    [ -f "$file" ] || return 0
    grep -q "#include <$header>" "$file" && return 0
    step "patching $(basename "$file") (needs <$header>)"
    sed -i.bak "1a #include <$header>" "$file"
}
# `std::function`.
patch_include lib/common/grid_util/BinPacketDictionary.h functional
# `std::sort` and `std::max_element`.
patch_include lib/common/grid_util/AffinityResourceControl.cc algorithm

# **The Python bindings are built unconditionally and are not wanted.**
# `mod/python/py_scene_rdl2` needs Boost.Python and a matching Python,
# which means dragging both into the environment for a module nothing
# here loads -- and pinning Boost against the version OpenImageIO and
# OSL were built with, which is the coupling worth avoiding. There is
# no CMake option, so the subdirectory is commented out.
#
# Unlike the `<functional>` patch above this is **not a bug**: it is an
# optional component being skipped, and a build that wants it should
# drop this and add `libboost-python-devel`.
MOD="$VENDOR/scene_rdl2/mod/CMakeLists.txt"
if [ -f "$MOD" ] && grep -q "^add_subdirectory(python)" "$MOD"; then
    step "skipping scene_rdl2's Python bindings"
    sed -i.bak 's|^add_subdirectory(python)|# skipped by packaging/renderer.sh: needs Boost.Python\n# add_subdirectory(python)|' "$MOD"
fi

# **GCC, explicitly, when it is there.** `/usr/bin/c++` is clang on
# some distributions, and the build above is only known to work with
# GCC -- `quickstart.md` verified it on 13.3. Letting CMake take
# whatever `c++` happens to be turns one missing include into an
# unknown number of them. Override with `$CC`/`$CXX` to use another.
TOOLCHAIN=""
if [ -z "${CXX:-}" ] && command -v g++ >/dev/null 2>&1; then
    TOOLCHAIN="-DCMAKE_C_COMPILER=$(command -v gcc) \
-DCMAKE_CXX_COMPILER=$(command -v g++)"
fi

step "scene_rdl2"
# **The Makefile generator, not Ninja.** `ISPC_HEADER_DIRECTORY` is set
# with a leading slash, so under Ninja the generated header is declared
# at the filesystem root while every consumer wants it in the build
# tree. Same under the CMake version MoonRay's own script downloads, so
# it is not a regression to wait out.
CMAKE_MODULES_ROOT="$MODULES" cmake -S "$VENDOR/scene_rdl2" -B "$VENDOR/build-rdl2" \
    -G "Unix Makefiles" $TOOLCHAIN \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DCMAKE_MODULE_PATH="$MODULES/cmake"
cmake --build "$VENDOR/build-rdl2" -j"$JOBS"
cmake --install "$VENDOR/build-rdl2"

# **Use the environment's OpenSubdiv when it has one, and do not build
# a second.** The pixi environment ships 3.7 and puts its headers on
# `CMAKE_PREFIX_PATH`, so a source build of 3.5 beside it gets compiled
# against 3.7 headers and linked against the 3.5 library. That fails at
# the very end, linking `moonray`, with an undefined reference naming a
# symbol in an `OpenSubdiv::v3_7_0` namespace -- an hour in, and it
# reads as a MoonRay bug rather than as two versions.
if [ -n "$PIXI_ENV" ] && [ -d "$PIXI_ENV/include/opensubdiv" ] \
   && [ -f "$PIXI_ENV/lib/libosdCPU.$SHLIB" ]; then
    OSD_INCLUDE="$PIXI_ENV/include/opensubdiv"
    OSD_LIBRARY="$PIXI_ENV/lib/libosdCPU.$SHLIB"
    step "OpenSubdiv (from the environment: $(basename "$OSD_LIBRARY"))"
else
    OSD_INCLUDE="$PREFIX/include/opensubdiv"
    OSD_LIBRARY="$PREFIX/lib/libosdCPU.$SHLIB"
    step "OpenSubdiv (from source)"
    fetch OpenSubdiv https://github.com/PixarAnimationStudios/OpenSubdiv.git "$OPENSUBDIV_REF"
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
fi

step "OpenImageDenoise (binary release)"
OIDN="oidn-$OIDN_VERSION.$OIDN_PLATFORM"
if [ ! -d "$VENDOR/$OIDN" ]; then
    curl -sSL -o "$VENDOR/$OIDN.tar.gz" \
      "https://github.com/OpenImageDenoise/oidn/releases/download/v$OIDN_VERSION/$OIDN.tar.gz"
    tar xzf "$VENDOR/$OIDN.tar.gz" -C "$VENDOR"
fi
cp -a "$VENDOR/$OIDN/include/." "$PREFIX/include/"
cp -a "$VENDOR/$OIDN/lib/." "$PREFIX/lib/"

step "mcrt_denoise"
CMAKE_MODULES_ROOT="$MODULES" cmake -S "$VENDOR/mcrt_denoise" -B "$VENDOR/build-denoise" \
    -G "Unix Makefiles" $TOOLCHAIN \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DCMAKE_PREFIX_PATH="$PREFIX" -DCMAKE_MODULE_PATH="$MODULES/cmake" \
    -DMOONRAY_USE_OPTIX=NO
cmake --build "$VENDOR/build-denoise" -j"$JOBS"
cmake --install "$VENDOR/build-denoise"

step "moonray"
# **`FindOpenSubDiv` wants an `osdGPU`** a CPU-only OpenSubdiv does not
# build, so the CPU library is named for both.
CMAKE_MODULES_ROOT="$MODULES" cmake -S "$VENDOR/moonray" -B "$VENDOR/build-moonray" \
    -G "Unix Makefiles" $TOOLCHAIN \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DCMAKE_PREFIX_PATH="$PREFIX" -DCMAKE_MODULE_PATH="$MODULES/cmake" \
    -DMOONRAY_USE_OPTIX=NO -DMOONRAY_BUILD_TESTING=NO \
    -DOpenSubDiv_INCLUDE_DIR="$OSD_INCLUDE" \
    -DOpenSubDiv_CPU_LIBRARY="$OSD_LIBRARY" \
    -DOpenSubDiv_GPU_LIBRARY="$OSD_LIBRARY"
# **`MOONRAY_BUILD_TESTING=NO` does not stop the test binaries being
# configured**, and two of them fail to link. Naming the target builds
# the renderer without them.
cmake --build "$VENDOR/build-moonray" -j"$JOBS" --target moonray
cmake --install "$VENDOR/build-moonray"

step "done"
[ -d "$PREFIX/rdl2dso" ] || die "$PREFIX/rdl2dso was not installed, so \
no scene class would resolve; the build above did not finish"

echo "renderer: $PREFIX"
echo "renderer: $(find "$PREFIX/rdl2dso" -name "*.$SHLIB" | wc -l | tr -d ' ') \
scene classes"
echo ""
echo "  just test-rdl2      the renderer tests"
echo "  just bundle $PREFIX"
