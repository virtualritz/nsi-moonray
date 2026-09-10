#!/usr/bin/env sh
#
# The environment every renderer recipe needs, in one place.
#
# Sourced, not run: `NSI_MOONRAY_PREFIX=... . packaging/env.sh`. Sets
# what `build.rs` and the shim read, and picks the pixi environment up
# when there is one so that a checkout with `just pixi-install` behind
# it gets OSL without anybody exporting anything.
#
# **The prefix arrives in the environment, not as `$1`.** POSIX leaves
# the behaviour of arguments to `.` unspecified and some shells drop
# them, which reads as the default silently winning -- a renderer built
# in one place and looked for in another.
#
# Each variable is left alone if already set, so an explicit one from
# the shell always wins.

PREFIX="${NSI_MOONRAY_PREFIX:-}"
if [ -z "$PREFIX" ]; then
    case "$(uname -s)" in
        Darwin) PREFIX="$HOME/Library/Application Support/MoonRay" ;;
        *)      PREFIX="${XDG_DATA_HOME:-$HOME/.local/share}/moonray" ;;
    esac
fi

# **One compiler across the shim, the OSL DSO and the renderer.**
# `packaging/renderer.sh` builds MoonRay with GCC because that is what
# the recipe was verified with, and `/usr/bin/c++` is clang on some
# machines -- so without this, `cc-rs` compiles `shim/src/scene.cc`
# with a different compiler than the library it links against. That is
# an ABI question nobody wants to have, and on clang 18 it does not
# even get that far: `scene_rdl2/scene/rdl2/Shader.h` subscripts a
# pointer to a type only MoonRay completes, which GCC accepts and
# clang rejects.
if [ -z "${CXX:-}" ] && command -v g++ >/dev/null 2>&1; then
    CC="$(command -v gcc)"
    CXX="$(command -v g++)"
    export CC CXX
fi

# The MoonRay install: scene classes and the renderer.
export SCENE_RDL2_ROOT="${SCENE_RDL2_ROOT:-$PREFIX}"
export MOONRAY_ROOT="${MOONRAY_ROOT:-$PREFIX}"
export NSI_MOONRAY_DSO="${NSI_MOONRAY_DSO:-$PREFIX/rdl2dso}"

# **Where a host looks for this backend**, the way `$DELIGHT` is where
# it looks for 3Delight: a prefix whose `lib` holds the ɴsɪ library.
# The `nsi` crate reads it when a `Context` asks for the renderer named
# `"moonray"`, so setting it is what makes a checkout selectable from
# an application without installing anything.
#
# Deliberately **not** `$MOONRAY_ROOT`, which names DreamWorks'
# renderer. This is the ɴsɪ front end onto it, they are installed
# separately often enough, and one variable for both would make "which
# MoonRay is this" unanswerable.
export NSI_MOONRAY="${NSI_MOONRAY:-$PWD/target/debug}"

# The pixi environment, if this checkout has one.
#
# **`$OSL_ROOT` is what turns OSL on.** Without it `build.rs` skips the
# DSO in `dso/osl/`, `cfg(osl)` is unset, and every shader becomes a
# `UsdPreviewSurface` -- a working render of the wrong materials. pixi
# is the only route that supplies OSL at all: neither Ubuntu nor
# Homebrew packages it.
PIXI_ENV="$PWD/.pixi/envs/default"
if [ -d "$PIXI_ENV" ]; then
    export OSL_ROOT="${OSL_ROOT:-$PIXI_ENV}"
    export CMAKE_PREFIX_PATH="${PIXI_ENV}${CMAKE_PREFIX_PATH:+:$CMAKE_PREFIX_PATH}"
    export PATH="$PIXI_ENV/bin:$PATH"
    # rdl2's `AsciiReader` includes `lua.hpp`. The conda package puts
    # it straight in `include`, not in an `include/lua5.3` the way
    # Debian does, which is what `build.rs` defaults to.
    export LUA_INCLUDE_DIR="${LUA_INCLUDE_DIR:-$PIXI_ENV/include}"
    # The conda libraries are not on the loader's path, and the
    # failure is at `dlopen` time naming a library nobody chose.
    export LD_LIBRARY_PATH="$PIXI_ENV/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export DYLD_LIBRARY_PATH="$PIXI_ENV/lib${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
fi

# The sample volume the volume tests render, if `just assets` has
# fetched it. Absent, those tests say why they did nothing.
if [ -z "${NSI_MOONRAY_VDB:-}" ] && [ -s "$PWD/vendor/assets/fire.vdb" ]; then
    export NSI_MOONRAY_VDB="$PWD/vendor/assets/fire.vdb"
fi
if [ -z "${NSI_MOONRAY_VDB_POINTS:-}" ] \
   && [ -s "$PWD/vendor/assets/waterfall_points.vdb" ]; then
    export NSI_MOONRAY_VDB_POINTS="$PWD/vendor/assets/waterfall_points.vdb"
fi

# MoonRay's own libraries, for the same reason.
export LD_LIBRARY_PATH="$PREFIX/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export DYLD_LIBRARY_PATH="$PREFIX/lib${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
