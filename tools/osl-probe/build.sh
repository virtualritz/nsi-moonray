#!/bin/sh
# Build and run the OSL probe.
#
# Usage: build.sh [osl-install-root]

set -e
osl=${1:-/home/user/osl/install}
here=$(cd "$(dirname "$0")" && pwd)
out=$here/build

mkdir -p "$out"

"$osl/bin/oslc" -o "$out/probe.oso" "$here/emitter.osl"

g++ -std=c++17 -O2 \
    -I"$osl/include" \
    "$here/probe.cc" -o "$out/probe" \
    -L"$osl/lib" -loslexec -loslcomp -lOpenImageIO -lOpenImageIO_Util

echo "built $out/probe"
