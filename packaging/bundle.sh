#!/usr/bin/env sh
#
# Assemble a relocatable nsi-moonray bundle: this backend, MoonRay, and
# every shared library the two need that the host cannot be assumed to
# have.
#
# The layout is a contract, not a convention. `src/dso.rs` looks for
# `../lib/rdl2dso` relative to the running binary before it looks
# anywhere else, so a bundle unpacked to any path finds its own scene
# classes with no environment set. Change one and change the other.
#
#     <root>/
#       bin/mnry              the front end
#       bin/moonray           the renderer, for the spawn fallback
#       lib/libnsi_moonray.*  the drop-in ɴsɪ renderer
#       lib/rdl2dso/          MoonRay's scene classes  <- dso.rs finds this
#       lib/                  the shared libraries both need
#       share/nsi-moonray/    README, licences
#
# This script only *assembles*. Building is the justfile's job, so that
# what is tested here is the layout and the dependency walk rather than
# a toolchain.
#
# Usage:
#   packaging/bundle.sh --prefix DIR --out DIR [--binary PATH]...
#
#   --prefix   a MoonRay install: the one `quickstart.md` builds, with
#              `rdl2dso/` and `bin/moonray` under it
#   --out      where the bundle is written; created, and refused if it
#              already has contents
#   --binary   an artefact to place in `bin/`, repeatable. Without any,
#              `target/release/mnry` is assumed.
#   --library  an artefact to place in `lib/`, repeatable. Without any,
#              `target/release/libnsi_moonray.*` is assumed.
#   --check    verify the inputs and print what would be done

set -eu

PREFIX=""
OUT=""
CHECK=0
BINARIES=""
LIBRARIES=""

die() {
    echo "bundle: $*" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)  PREFIX="${2:-}"; shift 2 ;;
        --out)     OUT="${2:-}"; shift 2 ;;
        --binary)  BINARIES="$BINARIES ${2:-}"; shift 2 ;;
        --library) LIBRARIES="$LIBRARIES ${2:-}"; shift 2 ;;
        --check)   CHECK=1; shift ;;
        -h|--help) sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *)         die "unknown argument $1" ;;
    esac
done

[ -n "$PREFIX" ] || die "--prefix is required; see --help"
[ -n "$OUT" ] || die "--out is required; see --help"

# **The two things a MoonRay prefix has to have.** Checked by name
# rather than assumed, because a prefix that is merely a directory
# produces a bundle that is merely a directory -- and the failure
# arrives at somebody else's render.
[ -d "$PREFIX/rdl2dso" ] || die "$PREFIX/rdl2dso is not a directory; \
that is MoonRay's scene classes, and a bundle without them renders \
nothing at all"

case "$(uname -s)" in
    Darwin) DSO_EXT="dylib"; RENDERER="$PREFIX/bin/moonray" ;;
    *)      DSO_EXT="so";    RENDERER="$PREFIX/bin/moonray" ;;
esac

# The checkout's pixi environment, if it has one. Its libraries are in
# the bundle by way of the dependency walk, so its licences belong in
# the bundle too.
PIXI_ENV="$PWD/.pixi/envs/default"
[ -d "$PIXI_ENV" ] || PIXI_ENV=""

[ -x "$RENDERER" ] || die "$RENDERER is not executable; it is the \
fallback for any host that cannot link the renderer, and ɴsɪ always \
returns an image"

[ -n "$BINARIES" ] || BINARIES=" target/release/mnry"
if [ -z "$LIBRARIES" ]; then
    LIBRARIES=" target/release/libnsi_moonray.$DSO_EXT"
fi

for artefact in $BINARIES $LIBRARIES; do
    [ -f "$artefact" ] || die "$artefact was not built; \`just bundle\` \
builds before it assembles"
done

if [ -d "$OUT" ] && [ -n "$(ls -A "$OUT" 2>/dev/null)" ]; then
    die "$OUT is not empty; a bundle assembled over another one carries \
whatever the last one left behind"
fi

if [ "$CHECK" -eq 1 ]; then
    echo "bundle: prefix   $PREFIX"
    echo "bundle: out      $OUT"
    echo "bundle: renderer $RENDERER"
    echo "bundle: binaries$BINARIES"
    echo "bundle: libraries$LIBRARIES"
    echo "bundle: classes  $(find "$PREFIX/rdl2dso" -name "*.$DSO_EXT" \
        | wc -l | tr -d ' ') scene classes"
    exit 0
fi

mkdir -p "$OUT/bin" "$OUT/lib/rdl2dso" "$OUT/share/nsi-moonray"

for artefact in $BINARIES; do
    cp -pL "$artefact" "$OUT/bin/"
done
cp -pL "$RENDERER" "$OUT/bin/"
for artefact in $LIBRARIES; do
    cp -pL "$artefact" "$OUT/lib/"
done

# The scene classes. `cp -R` of the directory's *contents*, so a prefix
# that keeps other things beside them does not smuggle them in.
find "$PREFIX/rdl2dso" -maxdepth 1 -name "*.$DSO_EXT" \
    -exec cp -pL {} "$OUT/lib/rdl2dso/" \;

# Everything MoonRay's own libraries live in, which is not on a host
# that never built it.
if [ -d "$PREFIX/lib" ]; then
    find "$PREFIX/lib" -maxdepth 1 \( -name "*.$DSO_EXT" -o -name "*.$DSO_EXT.*" \) \
        -exec cp -pL {} "$OUT/lib/" \;
fi

# **The transitive walk.** A bundle that carries MoonRay and not
# OpenImageIO is a bundle that fails on the first host without it, at
# `dlopen` time, with a message naming a library nobody chose.
#
# The deny-list is the interface between a process and its kernel and
# libc, which cannot be relocated and must not be shipped. Everything
# else travels: `libstdc++` included, because MoonRay wants a newer one
# than several supported distributions carry.
system_library() {
    case "$1" in
        linux-vdso.so*|ld-linux*|/lib*/ld-*) return 0 ;;
        libc.so*|libm.so*|libdl.so*|librt.so*|libpthread.so*) return 0 ;;
        libresolv.so*|libgcc_s.so*) return 0 ;;
        /usr/lib/libSystem*|/usr/lib/libc++*|/System/*) return 0 ;;
        *) return 1 ;;
    esac
}

dependencies() {
    case "$(uname -s)" in
        Darwin) otool -L "$1" 2>/dev/null | tail -n +2 | awk '{print $1}' ;;
        *)      ldd "$1" 2>/dev/null | awk '/=>/ {print $3} /^\t\// {print $1}' ;;
    esac
}

collect() {
    subject="$1"
    for dependency in $(dependencies "$subject"); do
        [ -f "$dependency" ] || continue
        name="$(basename "$dependency")"
        system_library "$name" && continue
        system_library "$dependency" && continue
        [ -f "$OUT/lib/$name" ] && continue
        cp -pL "$dependency" "$OUT/lib/$name"
        collect "$OUT/lib/$name"
    done
}

for subject in "$OUT"/bin/* "$OUT"/lib/*."$DSO_EXT" \
               "$OUT"/lib/rdl2dso/*."$DSO_EXT"; do
    [ -f "$subject" ] || continue
    collect "$subject"
done

# **Relocatable, or the bundle is a list of files in the wrong place.**
# Without this every binary keeps the absolute rpath it was linked
# with, which points at a build tree that does not exist on the host.
relocate() {
    subject="$1"
    origin="$2"
    case "$(uname -s)" in
        Darwin)
            install_name_tool -add_rpath "@executable_path/$origin" \
                "$subject" 2>/dev/null || true
            ;;
        *)
            command -v patchelf >/dev/null 2>&1 || return 0
            patchelf --set-rpath "\$ORIGIN/$origin" "$subject" || true
            ;;
    esac
}

for subject in "$OUT"/bin/*; do
    [ -f "$subject" ] && relocate "$subject" "../lib"
done
for subject in "$OUT"/lib/*."$DSO_EXT"; do
    [ -f "$subject" ] && relocate "$subject" "."
done
for subject in "$OUT"/lib/rdl2dso/*."$DSO_EXT"; do
    [ -f "$subject" ] && relocate "$subject" ".."
done

if [ "$(uname -s)" != "Darwin" ] && ! command -v patchelf >/dev/null 2>&1; then
    echo "bundle: patchelf is not installed, so nothing was made \
relocatable -- this bundle works only where it was built" >&2
fi

cp -pL README.md "$OUT/share/nsi-moonray/" 2>/dev/null || true
for notice in LICENSE.md LICENSE-MIT LICENSE-APACHE LICENSE-ZLIB; do
    [ -f "$notice" ] && cp -pL "$notice" "$OUT/share/nsi-moonray/"
done

# **Other people's software travels with other people's notices.** A
# bundle carries MoonRay, Embree, OpenVDB, OpenImageIO, OSL and a dozen
# more, and redistributing a library means redistributing its licence.
# This is a best effort over the places the sources put them -- it is
# not a legal opinion, and `LICENSE.md` says so.
LICENCES="$OUT/share/nsi-moonray/licences"
mkdir -p "$LICENCES"
found=0
for root in "$PREFIX" "$PIXI_ENV"; do
    [ -n "$root" ] && [ -d "$root" ] || continue
    # conda writes `share/info/licenses` per package; a `make install`
    # tends to drop them in `share/doc/<name>`.
    for candidate in "$root"/share/info/licenses/* "$root"/share/doc/*; do
        [ -e "$candidate" ] || continue
        name="$(basename "$candidate")"
        [ -e "$LICENCES/$name" ] && continue
        cp -RpL "$candidate" "$LICENCES/$name" 2>/dev/null || continue
        found=$((found + 1))
    done
done

if [ "$found" -eq 0 ]; then
    echo "bundle: no third-party licences were found to copy. A bundle \
redistributes MoonRay and its dependencies, so this needs checking by \
hand before the build goes anywhere." >&2
else
    echo "bundle: $found third-party licence directories collected"
fi

echo "bundle: $OUT"
echo "bundle: $(find "$OUT/lib/rdl2dso" -type f | wc -l | tr -d ' ') \
scene classes, $(find "$OUT/lib" -maxdepth 1 -type f | wc -l | tr -d ' ') \
libraries"
