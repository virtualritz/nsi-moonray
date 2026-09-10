# What Is Missing To Replace 3Delight In A DCC

A gap analysis: what stands between this backend and being a drop-in
ɴsɪ renderer for Houdini or Gaffer.

Read against the specification's node reference, this crate's source,
and the three real ɴsɪ client implementations -- Gaffer's
`IECoreDelight`, 3Delight for Houdini, and `hdNSI`. Where a claim is
about what a DCC does, it is from that client's source rather than
from its documentation.

**The dispatch that decides most of this is one match**, at
`src/flush.rs:372-589`. A node type absent from it is skipped and
reported; an attribute absent from the arm that handles its node is
dropped in silence. The second is the dangerous half.

## The pattern worth calling out first

The stated contract is that nothing refuses a scene and what cannot be
carried is recorded in `Flushed::limitations` (`flush.rs:9-16`). That
is honoured scrupulously for everything the flush *looks at*.

The dangerous class is what it never looks at: `visibility.*`, `matte`,
`nholes`, `environment.angle`, the whole `.global` node, and every
`screen` and `outputlayer` attribute beyond the handful mapped. Those
produce a plausible image of the wrong scene with nothing saying so --
exactly the failure this repository's design notes exist to prevent.

**That class is precisely where both DCCs live.** Gaffer's eight
per-object attributes, its `dl:` globals and its extra `screen` and
`outputlayer` attributes are all set, all ignored, and none reported.

So the cheapest first move, independent of implementing any of them,
is to sweep unknown attributes on the node types the flush *does*
handle and report them by name.

## Ranked blockers

1. **No display driver.** `DspyRegisterDriver` is a stub
   (`capi.rs:846`), `DspyRegisterDriverTable` is not exported at all,
   and `outputdriver.drivername` is ignored. The interface leaves the
   driver API unspecified and in practice everyone uses RenderMan's
   Dspy: Gaffer registers `ieDisplay`, Houdini registers its viewport
   driver and `idisplay`. **No viewport in either DCC receives a
   pixel by any route.** The working delivery path is Rust closures
   (`display.rs:26-35`), which needs the host to share a compilation
   of `nsi-ffi-wrap` -- something a C++ host cannot do.
2. **`NSIBegin` discards `errorhandler`** (`capi.rs:344`). Every
   diagnostic, including the fifty-odd limitation strings this crate
   writes so carefully, goes to stderr and never reaches the host's
   message log. This gates the usefulness of everything else: a user
   cannot see *why* a render is wrong. A few lines to fix.
3. **The `attributes` node is unread** (`flush.rs:538`). Only its
   shader bindings are consumed. `visibility.*`, `matte`, `bounds`
   and the rest are dropped silently. Upstream already resolves the
   full inheritance model -- `Scene::attribute_value`, with **zero
   call sites here** -- and `flush.rs:204-214` already lists MoonRay's
   nine visibility flags, used only to implement "disconnected from
   `.root`". Close to pure wiring.
4. **`NSIConnect` discards its arguments** (`capi.rs:485`). This
   corrupts rather than limits. `index` on `instances.sourcemodels`
   decides which prototype an instancer places, and
   `flush.rs:1030-1040` resolves pairing against it -- so through the
   C API a multi-prototype instancer places the **wrong models,
   silently**. `priority` on an `attributes` connection decides which
   shader wins a tie, which Houdini relies on with four distinct
   values. `connect_with_arguments` exists upstream, unused.
5. **`curves` and `particles` are unsupported** (`flush.rs:585`). Hair,
   fur, grass, point clouds. Both DCCs emit them, and
   `RdlCurveGeometry` and `RdlPointGeometry` are installed and unused.
6. **Six emitter names become lights** (`flush.rs:2263`). Anything else
   renders black with one line of explanation. Falloff, spread, barn
   doors, IES profiles, area shape and texture maps are lost even for
   the six.
7. **`NSIEvaluate` is empty** (`capi.rs:550`). Procedurals, archives
   and delayed load all silently do nothing.
8. **`.global` is entirely unread.** Zero of its 43 attributes. No
   sample counts, ray depths or thread count can be set, and Gaffer
   forwards its whole quality interface here.
9. **`lightdepth "auto"` is unhandled.** Both DCCs name layers
   `diffuse.direct` and `Ci.indirect` and rely on the suffix being
   stripped and the light depth applied. `lobe_label` (`flush.rs:3000`)
   matches exactly, so the standard AOV pass renders the beauty.
10. **`id.*` builtin AOVs are unmapped.** Neither DCC sets
    `cryptomatte.*`; they emit `id.geometry`, `id.scenepath` and
    `id.surfaceshader`. `state_variable` (`flush.rs:3015`) knows six
    names, none of them these, so object IDs and Cryptomatte are
    unreachable.
11. ~~**No light linking, shadow linking or light groups.**~~
    **Done.** ɴsɪ has no light-linking attribute -- §4.5 defers to
    §4.8's inter-object visibility, a cross-hierarchy connection into
    another object's `visibility` carrying a `"value"`. Three shapes of
    it map, and which column they land in is decided by the ray type
    and by which end is a light: geometry into a light's `visibility`
    is a per-row `LightSet` built by subtraction; a light into a
    shape's `visibility.shadow` is a `ShadowSet`; shape into shape is a
    `ShadowReceiverSet`. The last two collect what is excluded rather
    than what is kept, because that is the polarity
    `ShadowLinking::canCastShadow` reads. Light groups were already
    done, as light-path expression labels. What is still unmapped is
    general per-pair visibility, which MoonRay has no counterpart for,
    and it is reported by pair.
12. **Interactive delivery is one RGBA layer from one driver**
    (`stream.rs:86`, `session.rs:205-210`), and `stoppedcallback` is
    not read, so a viewport must poll.
13. **`faceset` is unsupported**, though `part_list` and
    `Assignment.part` both already exist.
14. **No environment texture** (`flush.rs:2566`), and
    `environment.angle` is unread, so a directional light renders as a
    full sphere.
15. **No depth of field, lens shift or shutter shape.** All three have
    MoonRay counterparts.
16. **Multi-camera scenes silently pick the wrong camera**, writing
    `output_file` and `camera` once per output into one
    `SceneVariables` (`flush.rs:590-632`), with the resolution taken
    from the first screen.

## What is fully carried

`transform`, `root` and `instances`. The last is the strongest part of
the backend: prototypes, per-instance matrices, model indices and
disabled instances all cross natively and nest five deep. Its one loss
-- a moving instancer blurs translation only, because rdl2's
`xform_list` is not blurrable -- is reported.

## What is partly carried

- **`mesh`** carries positions with motion, indices, winding,
  subdivision scheme, creases, corners, `st`, `N` and arbitrary
  primitive variables in all four interpolations. It silently ignores
  `nholes`, so a mesh with holes renders them solid, and
  `quadraticmotion`, so three-sample curved blur becomes linear.
- **The cameras** carry transform, clipping range, field of view and
  the fisheye mapping. Depth of field, lens shift, shutter shape and
  the lens shader connection are all silent.
- **`screen`** has two of ten attributes read.
- **`outputlayer`** carries the name, the variable and its source, and
  the scalar format. Twelve attributes are silent, and a layer fanning
  out to several drivers loses all but the first without a word.
- **`outputdriver`** reads only `imagefilename`.
- **`environment`** becomes a white `EnvLight` with a transform.
- **`shader`** is genuinely good with OSL running: the network crosses
  as a group specification, validated against the compiled shader
  first, with three classes of mistake reported by handle.
- **`volume`** carries the file and its grids, and as of the OSL
  volume shader a bound `volumeshader` runs.

## What is not carried at all

`curves`, `particles`, `faceset`, `set`, `plane`, `procedural`,
`vdbparticles`, `global`, `cylindricalcamera`, `nurbs`, `t-nurcc`.

All are reported rather than dropped. Three of them are genuine
renderer gaps rather than unimplemented mapping: `vdbparticles` needs
an OpenVDB point-grid reader MoonRay does not have,
`cylindricalcamera` has no MoonRay projection, and `nurbs` and
`t-nurcc` are draft in the specification and implemented by nobody.
The rest exist on MoonRay's side and are waiting to be wired.

## Two that are correct as they stand

`suspend` and `resume` are deliberately unmapped (`capi.rs:632-636`),
and Gaffer's own source says 3Delight does not support them either.
The interactive sequence every real client uses -- `start` with
`interactive`, `synchronize` per edit, `stop` -- is exactly what this
backend implements.
