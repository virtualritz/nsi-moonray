# Requirements Checklist: MoonRay Backend

## Spec Quality

- [x] User stories are independently testable.
- [x] Acceptance criteria are observable.
- [x] Non-goals are explicit.
- [x] Risks are named.

## Contract Quality

- [x] Every important behavior has a contract row.
- [x] Every row is `Covered`, `Partial`, or `Open`.
- [x] `Covered` rows cite source evidence. (None yet; all rows `Open`.)
- [x] `Covered` rows cite test or manual QA evidence. (None yet.)
- [x] Required evidence is listed before work starts.

## Implementation Readiness

- [x] Tasks are small enough for single commits.
- [ ] **A build host exists.** Unmet.
- [ ] **A binding strategy is chosen.** Unmet, and it gates scoping.

## Honesty Audit

- [x] Every row is `Open`. Nothing is implemented.
- [x] Renderer findings in `research.md` cite the file they came from,
      because they were read from a shallow clone rather than from
      documentation.
- [x] Time-to-first-pixel is **not** claimed at parity with 3Delight.
      The render modes were read; no benchmark was run.
- [x] The binding strategy is recorded as undecided rather than assumed
      to mirror the Mitsuba backend.
