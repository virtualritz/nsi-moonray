# Requirements Checklist: MoonRay Backend

## Spec Quality

- [x] User stories are independently testable.
- [x] Acceptance criteria are observable.
- [x] Non-goals are explicit.
- [x] Risks are named.

## Contract Quality

- [x] Every important behavior has a contract row.
- [x] Every row is `Covered`, `Partial`, or `Open`.
- [x] `Covered` rows cite source evidence.
- [x] `Covered` rows cite test or manual QA evidence.
- [x] Required evidence is listed before work starts.

## Implementation Readiness

- [x] Tasks are small enough for single commits.
- [x] **A build host for `scene_rdl2` exists.** Met: four cores, stock
      packages, about fifteen minutes.
- [ ] **A host that can build `moonray` exists.** Unmet. Only the
      rendering rows need it.
- [x] **A binding strategy is chosen.** Generate `.rdla`; settled by
      experiment.
- [ ] **`nsi-intermediate` can be depended on.** Unmet, and it gates
      the whole flush layer. `T0.7`.

## Honesty Audit

- [x] Only rows with both source and test evidence are `Covered`;
      everything about rendering is still `Open`, because nothing has
      been rendered.
- [x] Two findings in `research.md` were **wrong** and are corrected in
      place rather than quietly dropped: `scene_rdl2` does need ISPC,
      and MoonRay has no `use velocity` flag.
- [x] Renderer findings in `research.md` cite the file they came from,
      because they were read from a shallow clone rather than from
      documentation.
- [x] Time-to-first-pixel is **not** claimed at parity with 3Delight.
      The render modes were read; no benchmark was run.
- [x] The binding strategy was settled by experiment, not by assuming
      it mirrors the Mitsuba backend -- and the shim it does not use is
      still recorded as where progressive rendering has to end up.
