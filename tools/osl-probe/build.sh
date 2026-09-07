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

# `liboslexec` needs LLVM at run time -- it JITs -- and OSL does not
# put an rpath on it, so a probe run from a test harness with no
# `LD_LIBRARY_PATH` would fail to load rather than fail to parse.
llvm_lib=$(llvm-config --libdir 2>/dev/null || echo /usr/lib/llvm-18/lib)

g++ -std=c++17 -O2 \
    -I"$osl/include" \
    "$here/probe.cc" -o "$out/probe" \
    -L"$osl/lib" -loslexec -loslcomp -lOpenImageIO -lOpenImageIO_Util \
    -Wl,-rpath,"$osl/lib" -Wl,-rpath,"$llvm_lib"

echo "built $out/probe"
