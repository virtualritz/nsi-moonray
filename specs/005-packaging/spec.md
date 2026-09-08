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

**A release workflow cannot run yet.** `nsi-intermediate` is a path
dependency on a sibling checkout, so there is nothing a CI runner
could check out that would build -- `T0.7`, and the same thing that
keeps this repository from having any CI at all. Upstream tagged
`nsi-intermediate` 0.1.0 on 2026-09-08; once it is on crates.io the
path dependencies become version requirements and a workflow modelled
on `virtualritz/akatela`'s -- a draft release, one job per platform,
`cargo packager`, assets uploaded to the tag -- is the remaining step.

**Building MoonRay in CI is the expensive part.** About fifty minutes
on four cores, plus OpenSubdiv and OpenImageDenoise, and
`quickstart.md` lists five packaging problems that stop it. A cached
prefix per platform, rebuilt on a MoonRay version bump rather than per
commit, is what makes a release job finish in minutes.

## Not done

- The release workflow, for the reason above.
- Code signing and notarisation. macOS will refuse an unsigned `.dmg`
  from the internet, and saying so in a README is not a substitute.
- A licence file. `Cargo.toml` says `MIT OR Apache-2.0 OR Zlib` and
  the repository has no `LICENSE`, so the packager has none to embed.
- MoonRay's own licence and those of everything the bundle carries.
  Shipping Embree, OpenVDB, OpenImageIO and TBB means shipping their
  notices, and `share/nsi-moonray/` is where they go.
