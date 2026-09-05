# Feature Spec: MoonRay Backend

## User Stories

### User Story 1: Render A Recorded Scene (P1)

As an ɴsɪ consumer, I want a recorded scene rendered by MoonRay, so that
I have a production-grade open-source renderer behind the same interface
as 3Delight.

**Acceptance Criteria**

- Given a recorded `nsi_intermediate::Scene` with geometry, a camera and
  a screen, when it is flushed and rendered, then an image is produced.
- Given a scene with two shapes and two distinct materials, when it is
  rendered, then each material is applied to the correct shape.

### User Story 2: Motion Blur (P1)

As an animator, I want motion blur, so that a moving scene renders as it
would in 3Delight.

**Acceptance Criteria**

- Given a `transform` node with two time samples, when rendered, then
  the shape is blurred along its path.
- Given a `mesh` whose `P` carries two time samples, when rendered, then
  the deformation is blurred.

This is the capability that distinguishes MoonRay from the Mitsuba
backend, which cannot blur at all.

### User Story 3: Subdivision Surfaces At The Limit (P2)

As a modeller, I want subdivision surfaces evaluated at their limit
surface with view-dependent tessellation, matching what 3Delight does.

**Acceptance Criteria**

- Given a `subdivisionmesh` node, when rendered, then vertices lie on
  the limit surface rather than on a subdivided cage.
- Given a screen-space error tolerance, when the camera moves closer,
  then tessellation refines.

## Non-Goals

- **OSL.** MoonRay has none either; a code search for
  `OpenShadingLanguage` across `OpenMoonRay/moonray` returns nothing.
  Its shading is `BsdfBuilder` / `BsdfComponent` / `MapApi` with
  ISPC-vectorised DSOs. Shared OSL work belongs in a separate surface,
  not here.
- **3Delight's `dl*` shaders.** Closed-source `.oso`; nothing runs them.
- **Building MoonRay in CI.** It is a CMake build over OpenVDB, Embree,
  ISPC and OpenImageIO, normally done in Docker.

## Requirements

- R1: The backend contains no ɴsɪ graph semantics; it consumes resolved
  facts from `nsi-intermediate`.
- R2: ɴsɪ `Type::Reference` never reaches MoonRay.
- R3: Motion samples are honoured, not discarded.
- R4: Analytic primitives stay analytic; nothing is tessellated that
  MoonRay would not tessellate itself.

## Risks

- **Build weight.** Heavier than Mitsuba's. Treat a capable host as a
  precondition, not a tuning matter.
- **Binding strategy is unproven.** `scene_rdl2` is C++ with a plugin
  and DSO system. Whether a hand-written `extern "C"` shim is the right
  shape here, as it is for Mitsuba, is an open question -- MoonRay also
  has `rdla` (Lua) as a scripted scene-authoring path, which may be a
  cheaper first target than C++ binding.
- **Silent material misbinding.** Inherited from `nsi-intermediate`'s
  top risk; needs its own two-material test here.
