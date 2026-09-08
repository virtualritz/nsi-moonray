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

# Needs `$SCENE_RDL2_ROOT`, and `$MOONRAY_ROOT` for anything past
# building a scene. `just env` says what is missing.

# Type-check the renderer half.
check-rdl2: require-rdl2
    cargo check --all-targets --features rdl2

# **`cargo test`, not `nextest`, and that is the whole point of this
# being a separate recipe.** MoonRay's driver state is process-global:
# `initGlobalDriver` sets up thread-local pools, the affinity manager
# and the image-write driver once per process, and two live
# `RenderContext`s abort inside the allocator rather than failing
# politely. `tests/inprocess.rs` and `tests/incremental.rs` serialize on
# a `static ONE_AT_A_TIME` mutex, and that only works where the tests
# are threads in one process. nextest gives each test its own process
# and the mutex goes inert -- .blueprints/base/test-runner-isolation.md.

# Run the tests that link MoonRay and render.
test-rdl2: require-rdl2
    cargo build --features rdl2 --lib
    cargo test --features rdl2

# Run this first when a renderer recipe does something surprising. A
# half-set environment is the usual cause, and the `cfg`s it selects
# are invisible in the error.

# Say what the renderer recipes can see.
env:
    @echo "SCENE_RDL2_ROOT = ${SCENE_RDL2_ROOT:-(unset) -- rdl2 will not build}"
    @echo "MOONRAY_ROOT    = ${MOONRAY_ROOT:-(unset) -- scene only, nothing renders}"
    @echo "OSL_ROOT        = ${OSL_ROOT:-(unset) -- shaders become UsdPreviewSurface}"
    @echo "NSI_MOONRAY_DSO = ${NSI_MOONRAY_DSO:-(unset) -- no MoonRay classes resolve}"
    @echo "LUA_INCLUDE_DIR = ${LUA_INCLUDE_DIR:-/usr/include/lua5.3 (default)}"

# Fail with the recipe rather than with a link error four minutes in.
[private]
require-rdl2:
    #!/usr/bin/env sh
    if [ -z "${SCENE_RDL2_ROOT:-}" ]; then
        echo "\$SCENE_RDL2_ROOT is unset, and the rdl2 feature needs it." >&2
        echo "specs/001-moonray-backend/quickstart.md builds the prefix." >&2
        echo "Then: export SCENE_RDL2_ROOT=\$PREFIX MOONRAY_ROOT=\$PREFIX" >&2
        exit 1
    fi

# Produce release artefacts.
build:
    cargo build --release

# Assemble a relocatable tree from a MoonRay install: this backend, the
# renderer, its scene classes and every shared library the two need.
# `packaging/bundle.sh --help` has the layout, and `tests/bundle.rs`
# holds that layout against the code that reads it.
#
# `patchelf` is what makes the result relocatable on Linux; without it
# the bundle works only where it was built, and the script says so.

# Assemble a bundle from a MoonRay install at PREFIX.
bundle PREFIX: require-rdl2
    cargo build --release --features rdl2
    rm -rf dist/bundle
    packaging/bundle.sh --prefix {{PREFIX}} --out dist/bundle

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
