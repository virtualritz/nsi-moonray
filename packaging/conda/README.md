# Conda Packages

Two separate things live under `packaging/`, and it is worth keeping
them apart:

- **`pixi.toml` at the repository root consumes** the
  [`aswf-pixi`](https://github.com/anderslanglands/aswf-pixi) channel.
  That works today. It is how this checkout gets Open Shading
  Language, which no system package manager provides.
- **This directory produces**, or would: a recipe so that
  `pixi add nsi-moonray` installs the backend with a renderer already
  built. Nothing here has been built.

## Why it is worth doing

The one command that still takes an hour is `just renderer`, and every
minute of it is compiling MoonRay. A conda package moves that
compilation to a build farm once per release, and the same lockfile
that resolves OSL today would resolve a renderer.

It also settles installation on macOS. `just bundle` produces a
relocatable tree and `cargo packager` wraps it, but a `.dmg` from the
internet has to be signed and notarised before macOS will open it. A
conda channel sidesteps that question rather than answering it.

## What has to exist first, in order

`nsi-moonray` links `scene_rdl2` and MoonRay, so its recipe cannot
build until theirs do:

| # | Package | State | The hard part |
| --- | --- | --- | --- |
| 1 | `scene_rdl2` | not written | ISPC as a CMake language; the Ninja generator cannot build it; a missing `<functional>` |
| 2 | `moonray` | not written | OpenSubdiv without TBB, a `FindOpenSubDiv` that wants a GPU library, test targets that configure and fail to link |
| 3 | `nsi-moonray` | drafted, `0.1.0/recipe.yaml` | needs 1 and 2 |

Every one of those five problems is already worked around in
`packaging/renderer.sh`, with the error each produces written up in
`specs/001-moonray-backend/quickstart.md`. That script is the build
script for recipes 1 and 2, not a starting point to rewrite.

**Linux and macOS only.** MoonRay's CMake handles Unix and Darwin and
has no MSVC path, so `win-64` is not a target for 1 or 2 even though
the channel supports it. `specs/005-packaging/spec.md` has the
evidence.

## Before opening anything

That repository's `AGENTS.md` asks for a conversation before a
package is converted or added -- specifically to agree the subpackage
breakdown and which outputs get published, rather than to review a
finished PR. Three things are worth settling first:

- Whether MoonRay belongs in an ASWF channel at all. It is an ASWF
  project, so probably; it is also much heavier than anything there
  now, and building it is most of the work.
- The split. This draft follows the channel's own convention --
  `-lib` for the drop-in renderer, `-tools` for `mnry`, and the plain
  name as a metapackage over both.
- Whether OSL is a hard dependency or a feature. Here it is hard: the
  substitution path exists for hosts that cannot have OSL, and a conda
  package is not one of those.

## The draft

`nsi-moonray/0.1.0/recipe.yaml`. It has a placeholder `sha256`, which
is filled in at the tag; rattler-build refuses a source without one,
which is correct and not to be worked around.
