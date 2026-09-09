# nsi-moonray justfile
# Run `just --list` to see all available commands.
#
# Recipe naming follows .blueprints/base/script-naming.md. Two things
# here are *not* what that template prescribes, and both are deliberate:
#
# - **No `--all-features`.** `rdl2` links `scene_rdl2`, and the reason
#   it is off by default is that the emitter, the flush and the oracle
#   are all checked without a renderer -- on a host that cannot build
#   MoonRay at all. `--all-features` would make every recipe demand one.
#   The renderer recipes are the `-rdl2` suffixed pair below.
# - **`test` builds the library first.** See `build-lib`.
#
# `just --list` shows the last comment line above a recipe, so the
# rationale for a recipe sits in a block separated by a blank line and
# the line touching the recipe is its summary.

# The MoonRay install every renderer recipe uses: `$SCENE_RDL2_ROOT`
# when it is set, otherwise the platform's per-user data directory,
# which is where `just renderer` puts one.
#
# **That location is not arbitrary.** `src/dso.rs` searches it, so a
# renderer installed there is found by an `mnry` from
# `cargo install --path .` with no flag and no environment. Building
# into the checkout would mean `--dso-path` forever.
prefix := env('SCENE_RDL2_ROOT', if os() == 'macos' {
    home_directory() / 'Library' / 'Application Support' / 'MoonRay'
} else {
    env('XDG_DATA_HOME', home_directory() / '.local' / 'share') / 'moonray'
})

# Default recipe: show available commands.
default:
    @just --list

# Common aliases.
alias t := test
alias c := check
alias f := fmt
alias l := lint

# Aggregate: what CI runs, with the non-fixing variants. Renderer-free,
# because that is the configuration CI can actually have.

# Run everything CI runs.
ci: fmt-check check lint-check test

# Compile every target without running anything.
check:
    cargo check --all-targets

# Run clippy with autofix (modifies working tree).
lint:
    cargo clippy --fix --allow-dirty --allow-staged --all-targets -- -D warnings

# Run clippy without fixing (CI-safe).
lint-check:
    cargo clippy --all-targets -- -D warnings

# Format code.
fmt:
    cargo fmt --all

# Verify formatting without writing.
fmt-check:
    cargo fmt --all -- --check

# **The stale-artefact guard.** `tests/dropin.rs` `dlopen`s
# `target/debug/libnsi_moonray.so` by path, so Cargo does not know the
# test depends on it and will happily run the *previous* build. A stale
# artefact never looks like a stale artefact -- it looks like a bug you
# have already fixed, and it cost two debugging sessions before it was
# understood (`specs/002-interactive-updates/research.md` F7). Every
# recipe that runs tests depends on this one.

# Build the library, so no test opens a stale `cdylib`.
build-lib:
    cargo build --lib

# `nextest` for the suite, then `cargo test --doc`, because nextest
# does not run doctests.

# Run every test that needs no renderer.
test: build-lib
    cargo nextest run --no-fail-fast
    cargo test --doc

# Run one test by name, without a renderer.
test-single TEST: build-lib
    cargo nextest run {{TEST}}

# Everything needed to render, from nothing. Roughly an hour, nearly
# all of it MoonRay. Re-runnable: `just renderer` picks up where a
# failed build stopped.
#
# **The dependencies come from pixi, and that is not a preference.**
# Open Shading Language is not packaged by Ubuntu -- its `libosl-dev`
# is a Shogi library -- nor by Homebrew, and without it every shader
# becomes a `UsdPreviewSurface`. The ASWF conda channel has it, built
# against a matching OpenImageIO. pixi also needs no root and resolves
# Linux and macOS from one lockfile. `pixi.toml` has the detail.

# Install the dependencies, then fetch and build MoonRay.
setup: pixi-install renderer

# Resolve and install the dependencies into `.pixi/`. No root.
pixi-install:
    #!/usr/bin/env sh
    if ! command -v pixi >/dev/null 2>&1; then
        echo "pixi is not installed. Use --git, not the plain name:" >&2
        echo "  cargo binstall --git https://github.com/prefix-dev/pixi pixi" >&2
        echo "crates.io still has 0.15.2; the current release is 0.80." >&2
        exit 1
    fi
    pixi install

# The system-package route instead of pixi: no OSL, and asks for sudo.
deps:
    packaging/deps.sh

# Print those system packages without installing anything.
deps-list:
    @packaging/deps.sh --list

# Fetch and build MoonRay into `vendor/install`.
renderer:
    packaging/renderer.sh --prefix "{{prefix}}"

# **The volume tests need a volume**, and one is an asset rather than
# something a checkout has. These are the OpenVDB project's own sample
# models, so the framing in `tests/inprocess.rs` is measured against a
# file anyone can fetch rather than taken on faith -- which is what
# kept the volume path from ever being exercised.
#
# About 38 MB, into `vendor/assets/`, which is gitignored.

# Download the OpenVDB sample models the volume tests use.
assets:
    #!/usr/bin/env sh
    set -eu
    mkdir -p vendor/assets
    base=https://media.githubusercontent.com/media/AcademySoftwareFoundation/openvdb-website/master/download/models
    for name in fire.vdb waterfall_points.vdb; do
        if [ -s "vendor/assets/$name" ]; then
            echo "assets: $name is already here"
            continue
        fi
        echo "assets: fetching $name"
        curl -sSL -o "vendor/assets/$name" "$base/$name"
    done
    ls -la vendor/assets/

# Check the tools and headers a renderer build needs, building nothing.
renderer-check:
    packaging/renderer.sh --prefix "{{prefix}}" --check

# Type-check the renderer half.
check-rdl2: require-rdl2
    #!/usr/bin/env sh
    set -eu
    NSI_MOONRAY_PREFIX="{{prefix}}" . packaging/env.sh
    cargo check --all-targets --features rdl2

# **`nextest`, and the first version of this recipe argued the
# opposite.** The reasoning was that `tests/inprocess.rs` serializes on
# a `static ONE_AT_A_TIME` mutex, which nextest's process-per-test
# model makes inert, so `cargo test` was the safe runner. Running it
# settled it the other way.
#
# What MoonRay actually requires is **one renderer per process** --
# `initGlobalDriver` sets up thread-local pools, the affinity manager
# and the image-write driver once per process. nextest gives exactly
# that by construction, so the mutex is not merely inert, it is
# unnecessary. And a renderer test that dies takes its process with
# it: under `cargo test` one SIGBUS lost fifteen tests that had not run
# yet and reported nothing about them, where nextest lost one and ran
# the other twenty-five.
#
# The general rule in .blueprints/base/test-runner-isolation.md still
# holds; the resource here just happens to be one a process boundary
# provides rather than one it breaks.

# Run the tests that link MoonRay and render.
test-rdl2: require-rdl2
    #!/usr/bin/env sh
    set -eu
    NSI_MOONRAY_PREFIX="{{prefix}}" . packaging/env.sh
    cargo build --features rdl2 --lib
    cargo nextest run --features rdl2 --no-fail-fast

# Run this first when a renderer recipe does something surprising. A
# half-set environment is the usual cause, and the `cfg`s it selects
# are invisible in the error.

# Say what the renderer recipes can see.
env:
    #!/usr/bin/env sh
    NSI_MOONRAY_PREFIX="{{prefix}}" . packaging/env.sh
    echo "pixi env        = $([ -d .pixi/envs/default ] && echo present || echo '(none) -- run just pixi-install')"
    echo "SCENE_RDL2_ROOT = $SCENE_RDL2_ROOT$([ -d "$SCENE_RDL2_ROOT/rdl2dso" ] || echo '  (no rdl2dso there)')"
    echo "MOONRAY_ROOT    = $MOONRAY_ROOT"
    echo "NSI_MOONRAY_DSO = $NSI_MOONRAY_DSO"
    echo "OSL_ROOT        = ${OSL_ROOT:-(unset) -- shaders become UsdPreviewSurface}"
    echo "LUA_INCLUDE_DIR = ${LUA_INCLUDE_DIR:-/usr/include/lua5.3 (default)}"

# Fail with the recipe rather than with a link error four minutes in.
[private]
require-rdl2:
    #!/usr/bin/env sh
    if [ ! -d "{{prefix}}/rdl2dso" ]; then
        echo "No MoonRay install at {{prefix}}." >&2
        echo "  just setup                 installs and builds one there" >&2
        echo "  SCENE_RDL2_ROOT=... just ...  uses one you already have" >&2
        exit 1
    fi

# Produce release artefacts.
build:
    cargo build --release

# Put `mnry` on the PATH from this checkout. With a renderer built --
# `just setup` -- this links it, so renders happen in process; without
# one it still installs, and falls back to spawning the `moonray`
# binary. Either way the scene classes are found with no flag, because
# `just renderer` installs them where `src/dso.rs` looks.

# Install `mnry` into ~/.cargo/bin.
install:
    #!/usr/bin/env sh
    set -eu
    NSI_MOONRAY_PREFIX="{{prefix}}" . packaging/env.sh
    if [ -d "{{prefix}}/rdl2dso" ]; then
        cargo install --path . --locked --features rdl2
        echo "mnry: installed with the renderer linked"
    else
        cargo install --path . --locked
        echo "mnry: installed without a renderer -- it will spawn the" >&2
        echo "      \`moonray\` binary. \`just setup\` builds one." >&2
    fi

# Assemble a relocatable tree from a MoonRay install: this backend, the
# renderer, its scene classes and every shared library the two need.
# `packaging/bundle.sh --help` has the layout, and `tests/bundle.rs`
# holds that layout against the code that reads it.
#
# `patchelf` is what makes the result relocatable on Linux; without it
# the bundle works only where it was built, and the script says so.

# Assemble a bundle from a MoonRay install (default: vendor/install).
bundle PREFIX=prefix: require-rdl2
    #!/usr/bin/env sh
    set -eu
    NSI_MOONRAY_PREFIX="{{PREFIX}}" . packaging/env.sh
    cargo build --release --features rdl2
    rm -rf dist/bundle
    packaging/bundle.sh --prefix "{{PREFIX}}" --out dist/bundle

# Wrap `dist/bundle` in the platform's own installer. Linux gets a DEB
# and an AppImage, macOS a DMG. There is no Windows target: MoonRay's
# build handles Unix and Darwin and has no MSVC path, so a Windows
# package would carry the emitter and no renderer -- see
# `specs/005-packaging/spec.md`.
#
# Needs `cargo binstall cargo-packager` once.

# Build the installers for this platform from `dist/bundle`.
package:
    #!/usr/bin/env sh
    set -eu
    if [ ! -d dist/bundle ]; then
        echo "dist/bundle does not exist; run \`just bundle PREFIX\` first." >&2
        echo "Packaging without it ships an installer with no renderer." >&2
        exit 1
    fi
    case "$(uname -s)" in
        Darwin) cargo packager --release --formats dmg ;;
        *)      cargo packager --release --formats deb,appimage ;;
    esac

# Build documentation.
doc:
    cargo doc --no-deps

# Build and open documentation in a browser.
doc-open:
    cargo doc --no-deps --open

# `just run-example polyhedron` is the drop-in path end to end, through
# an unmodified ɴsɪ consumer.

# Run an example.
run-example EXAMPLE:
    cargo run --example {{EXAMPLE}}

# Run the `mnry` front end: `just mnry render scene.nsi`.
mnry *ARGS:
    cargo run --bin mnry -- {{ARGS}}

# Run a security audit.
audit:
    cargo audit
