# Quickstart: MoonRay Backend

## Status

Nothing to run. This repository is spec-only.

## Prerequisites, When Work Starts

- A host that can build MoonRay: a CMake build over OpenVDB, Embree,
  ISPC and OpenImageIO. Docker is the documented path.
- [`nsi`](https://github.com/virtualritz/nsi) for `nsi-intermediate`.

## The First Thing To Decide

Not a command -- a choice. Read `research.md` Open Questions and settle
the binding strategy: an `extern "C"` shim over `scene_rdl2`, or
generating `.rdla`. Every task depends on which.

Emitting `.rdla` reuses the stream emitter that already exists in
`nsi-intermediate`, so it is the cheaper route to a first image. It is
also a batch authoring path, so it cannot reach MoonRay's progressive
modes. Choose knowing that.

## Verification Commands

None yet.
