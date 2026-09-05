# The `.rdla` Format Oracle

Builds small scenes through the real `scene_rdl2` and writes them out
with rdl2's own `AsciiWriter`. The result, checked in under
`specs/001-moonray-backend/oracle/`, is what this backend's emitter is
tested against.

This exists because the `.nsi` emitter upstream is correct precisely
because 3Delight's own output was read first, and reading it corrected
four assumptions that would each have shipped a plausible, wrong file.
The same discipline applies here.

Needs `scene_rdl2` only — no renderer. Build recipe and how to run it:
`specs/001-moonray-backend/quickstart.md`.

The four scenes:

| Scene | Pins down |
| --- | --- |
| `types` | one attribute per `AttributeType`, and how numbers print |
| `scene` | geometry, a `Layer` assignment, sets, `RenderOutput`, `SceneVariables` |
| `blur` | `blur(a, b)` — rdl2 takes exactly two motion samples |
| `binding` | `bind(...)`, where ɴsɪ's named shader ports land |

Values are chosen to differ from each class's declared defaults, because
the writer runs with `setSkipDefaults(true)` and would otherwise omit
the very attribute being captured.
