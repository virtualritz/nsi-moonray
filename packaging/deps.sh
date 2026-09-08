#!/usr/bin/env sh
#
# Every system package building MoonRay needs, in one command.
#
# The list is the union of what `scene_rdl2` asks for and what MoonRay
# adds, verified against Ubuntu 24.04's archive rather than copied from
# a wiki. Two things are deliberately *not* here, and both are traps:
#
# - **Open Shading Language is not packaged.** Ubuntu's `libosl-dev` is
#   a library for Shogi programs. OSL has to be built from source, and
#   it is optional: without `$OSL_ROOT` this backend substitutes
#   `UsdPreviewSurface` for every shader and says so.
# - **OpenSubdiv and OpenImageDenoise** are not in the archive either.
#   `packaging/renderer.sh` builds the first and downloads the second.
#
# Usage: packaging/deps.sh [--list]
#
#   --list   print the packages and exit, installing nothing

set -eu

# `scene_rdl2`. ISPC is the one a reading of the docs misses: five
# library sources are `.ispc` and CMake adds ISPC to `project(...
# LANGUAGES ...)`.
RDL2="libboost-all-dev liblua5.3-dev lua5.3 libcppunit-dev libssl-dev \
libjsoncpp-dev liblog4cplus-dev libtbb-dev python3-dev ispc"

# MoonRay on top of it.
MOONRAY="libembree-dev libopenvdb-dev libopenimageio-dev \
openimageio-tools libopenexr-dev libimath-dev librandom123-dev \
libjpeg-dev zlib1g-dev libblosc-dev bison flex libcurl4-openssl-dev \
libmicrohttpd-dev"

# Building and bundling. `patchelf` is what makes a bundle relocatable;
# without it `packaging/bundle.sh` produces a tree that works only
# where it was built, and says so.
TOOLS="build-essential cmake git curl patchelf pkg-config"

PACKAGES="$RDL2 $MOONRAY $TOOLS"

if [ "${1:-}" = "--list" ]; then
    for package in $PACKAGES; do echo "$package"; done
    exit 0
fi

if ! command -v apt-get >/dev/null 2>&1; then
    echo "deps: this list is Debian/Ubuntu. On macOS the equivalent is \
Homebrew -- boost lua cppunit jsoncpp log4cplus tbb ispc embree \
openvdb openimageio openexr imath c-blosc bison flex -- and neither \
that nor MoonRay on macOS has been run from here." >&2
    exit 1
fi

echo "deps: installing $(echo "$PACKAGES" | wc -w | tr -d ' ') packages"
# Not run under `sudo` from inside: a script that elevates itself is a
# script you have to read before trusting, and this one wants to be
# read anyway. `--list` prints exactly what it would install.
sudo apt-get install -y $PACKAGES
