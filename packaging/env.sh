#!/usr/bin/env sh
#
# The environment every renderer recipe needs, in one place.
#
# Sourced, not run: `. packaging/env.sh <prefix>`. Sets what `build.rs`
# and the shim read, and picks the pixi environment up when there is
# one so that a checkout with `just pixi-install` behind it gets OSL
# without anybody exporting anything.
#
# Each variable is left alone if already set, so an explicit one from
# the shell always wins.

PREFIX="${1:-}"
[ -n "$PREFIX" ] || PREFIX="$PWD/vendor/install"

# The MoonRay install: scene classes and the renderer.
export SCENE_RDL2_ROOT="${SCENE_RDL2_ROOT:-$PREFIX}"
export MOONRAY_ROOT="${MOONRAY_ROOT:-$PREFIX}"
export NSI_MOONRAY_DSO="${NSI_MOONRAY_DSO:-$PREFIX/rdl2dso}"

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

# MoonRay's own libraries, for the same reason.
export LD_LIBRARY_PATH="$PREFIX/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export DYLD_LIBRARY_PATH="$PREFIX/lib${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
