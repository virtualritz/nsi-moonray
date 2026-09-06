# Feature Spec: Interactive Updates

## Why This Exists

An application drives a viewport: the user drags a slider, one
attribute on one node changes, and the render restarts *from what the
renderer already has*. ɴsɪ has a word for this —
`NSIRenderControl "synchronize"` — and a scene format cannot express it.

`001` chose to emit `.rdla` and spawn MoonRay's binary. That works for a
frame and cannot work for a viewport: every edit is a whole new scene
and a renderer that starts from nothing.

MoonRay supports the real thing, at a finer grain than ɴsɪ asks for.
See `research.md`.

## User Stories

### User Story 1: One Attribute Changes (P1)

As an application with a viewport, I want to change one attribute and
have the render restart without rebuilding the scene, so that
interaction is interactive.

**Acceptance Criteria**

- Given a rendering scene, when one shader parameter changes and the
  context is synchronised, then the image reflects it and no geometry
  is re-tessellated.
- Given the same, when one geometry's transform changes, then only that
  geometry's accelerator entry is rebuilt.

### User Story 2: Geometry Is Turned Off (P1)

As an application, I want to sever a piece of geometry's connection to
`.root` and have it disappear from the render, without the rest of the
scene being rebuilt.

**Acceptance Criteria**

- Given a rendering scene with two shapes, when one is disconnected and
  the context synchronised, then it is gone from the image and the
  other shape's tessellation is untouched.

### User Story 3: Pixels Without A File (P1)

As an application, I want rendered pixels delivered to my display
driver as they converge, not written to disk and read back.

**Acceptance Criteria**

- Given a progressive render, when samples land, then the registered
  display driver receives them.

## Non-Goals

- **`.rdla` as the transport.** It stays as an *output*: a scene dump
  for debugging and for batch, exactly as `.nsi` stream output is for
  the upstream recorder. It is no longer the path to the renderer.
- **Out-of-process rendering.** MoonRay has a binary delta format
  (`research.md` F4) and arras uses it. That is a later option and
  changes none of the mapping.

## Requirements

- R1: An ɴsɪ edit reaches MoonRay as an edit, not as a new scene.
- R2: The cost of an edit is the cost MoonRay assigns it. Turning
  geometry off must not re-tessellate it (`research.md` F3).
- R3: The backend holds no ɴsɪ graph knowledge. Working out *which*
  rdl2 objects an ɴsɪ edit touches is `nsi-intermediate`'s job, as
  composition and dissolution already are.
- R4: ɴsɪ always returns an image. An edit that cannot be applied
  incrementally falls back to a full rebuild and says so; it never
  fails the synchronise.

## Risks

- **Silent staleness.** An edit that is applied to the scene but does
  not mark the right objects changed renders the *previous* state, with
  no error. This is the interactive twin of `001`'s silent material
  misbinding, and it needs the same kind of test: change one thing,
  assert the pixels changed.
- **Cost regressions are invisible.** A mapping that re-tessellates
  where it could rebuild an accelerator looks correct and feels slow.
  Timing has to be part of the evidence, not an afterthought.
- **The upstream dependency.** Without a journal in
  `nsi-intermediate`, this backend can only diff whole scenes — which
  is a fallback, not a design.
