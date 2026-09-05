# Specs

Feature specs live here. The active feature directory is
`.specify/feature.json`.

## Index

| # | Surface | Status |
| --- | --- | --- |
| [001](001-moonray-backend/) | MoonRay backend | `.rdla` emitter, checked against a captured format oracle; the flush is blocked on depending on `nsi-intermediate` |

## Scope Of This Repository

This repository owns **only the flush into MoonRay**. Recording an ɴsɪ
scene, classifying its connections and resolving ɴsɪ's graph semantics
all happen upstream in
[`nsi-intermediate`](https://github.com/virtualritz/nsi), and are shared
with the [Mitsuba backend](https://github.com/virtualritz/nsi-mitsuba).

If a behaviour is wanted by every backend, it belongs upstream, not
here.
