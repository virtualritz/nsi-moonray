# Packaging: An Install That Needs No Environment

## The problem

Every object in a MoonRay scene is a class loaded from `rdl2dso`.
Until now the only way to name that directory was `--dso-path` or
`$NSI_MOONRAY_DSO`, and a drop-in renderer reached by `dlopen` has no
command line to be told on at all.

The failure is the worst shape this backend has: no class means no
object, no object means an empty `Layer`, and an empty `Layer` renders
a **black frame with no error anywhere**. Someone who installs a
package and gets a black image has no thread to pull.

So an install has to find itself.

## The layout, which is a contract

`packaging/bundle.sh` assembles this and `src/dso.rs` reads it.
`tests/bundle.rs` runs the script and holds the result against the
same function the renderer calls, because nothing else connects the
two and a disagreement is silent.

```
<root>/
  bin/mnry              the front end
  bin/moonray           the renderer, for the spawn fallback
  lib/libnsi_moonray.*  the drop-in ɴsɪ renderer
  lib/rdl2dso/          MoonRay's scene classes
  lib/                  every shared library the two need
  share/nsi-moonray/    README, licences
```

Four layouts are recognised, and each is one something here actually
produces rather than a guess:

| Layout | Where the classes are | Produced by |
| --- | --- | --- |
| Bundle | `../lib/rdl2dso` | `just bundle` |
| Flat | `./rdl2dso` | a zip unpacked in place |
| Installed package | `../lib/<binary>/lib/rdl2dso` | `cargo packager` |
| macOS app | `../Resources/lib/rdl2dso` | `cargo packager` |

**The third was found by building a `.deb` and listing it.** The
packager installs the binary to `/usr/bin/mnry` and every resource
under `/usr/lib/mnry/`, so the bundle layout misses an installed
package entirely -- which would have shipped an installer whose
renderer resolved nothing. It is pinned in `tests/bundle.rs` because
the packager could move it and nothing else would notice.

Explicit paths still win, and are not second-guessed: a `--dso-path`
that is not there fails rather than falling back to an install nobody
asked for. Rendering with a renderer you did not choose is the worse
outcome, and "it rendered, but not with the build I pointed at" is a
worse afternoon than "it did not render".

## Three platforms, two renderers

**This is the finding, and it is not a gap in the work.**

- **Linux.** Everything. `.deb` and AppImage.
- **macOS.** Everything. `.dmg`. MoonRay's CMake has a Darwin path and
  a Metal one, so the renderer builds; nothing here has run it.
- **Windows.** **No renderer.** MoonRay does not build there: its
  CMake handles Unix and Darwin and has no MSVC path anywhere, and
  `scene_rdl2` is compiled with `PLATFORM_UNIX`/`PLATFORM_LINUX`.
  `scene_rdl2`'s `Platform.h` carries Windows preprocessor branches
  inherited from Embree, which is not the same as a build.

  A Windows package could carry this crate's own half -- recording a
  scene, the flush, `mnry cat`, `.nsi` to `.rdla` -- and no renderer.
  That is a useful thing and a *different* product, and it should be
  named as one rather than shipped as a bundle that turns out to be
  empty. There is deliberately no `wix` section in `Cargo.toml`.

## `patchelf` is what makes it relocatable

Without it every binary keeps the absolute rpath it was linked with,
pointing at a build tree the host does not have. The script sets
`$ORIGIN`-relative rpaths where `patchelf` is available and **says so
on standard error when it is not**, rather than producing a bundle
that works only on the machine that made it and looks fine until it
moves. macOS uses `install_name_tool`, which ships with Xcode.

## What is blocked, and on what

**A release workflow is now possible.** It was blocked on
`nsi-intermediate` being a path dependency, so nothing a CI runner
checked out would build. That closed on 2026-09-08 when the crate
reached crates.io -- `T0.7`. The remaining step is a workflow modelled
on `virtualritz/akatela`'s: a draft release, one job per platform,
`cargo packager`, assets uploaded to the tag.

**Building MoonRay in CI is the expensive part.** About fifty minutes
on four cores, plus OpenSubdiv and OpenImageDenoise, and
`quickstart.md` lists five packaging problems that stop it. A cached
prefix per platform, rebuilt on a MoonRay version bump rather than per
commit, is what makes a release job finish in minutes.

## Written, not yet run

`.github/workflows/ci.yml` runs the renderer-free half on Linux and
macOS -- a minute, no MoonRay. `release.yml` drafts a release, builds
or restores a cached renderer per platform, runs the renderer tests,
bundles and packages.

The cache key is `packaging/renderer.sh` plus `pixi.lock`, which is
what actually determines the build: a release that does not move the
pinned refs or the dependency set restores the prefix and finishes in
minutes, and one that does pays the hour once.

Neither has run. Both are modelled on `virtualritz/akatela`'s, which
does.

## Not done

- **Code signing and notarisation.** Gatekeeper refuses an unsigned
  `.dmg` downloaded from the internet, so the macOS asset is usable by
  someone who clears the quarantine attribute and by nobody else. It
  needs an Apple Developer ID in the repository secrets and
  `xcrun notarytool` after the packager. The release workflow says so
  at the step that would do it.
- **Checksums for the release assets.**
- **macOS has never been exercised.** The dependency environment
  resolves for `osx-arm64` and the scripts have Darwin paths
  throughout -- `install_name_tool` rather than `patchelf`, the
  `arm64.macos` OpenImageDenoise build, `log4cplus` from source because
  conda-forge has no `osx-arm64` build of it. None of that has run.
  Treat the first macOS release as a bring-up.
