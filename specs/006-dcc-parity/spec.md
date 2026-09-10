# What Is Missing To Replace 3Delight In A DCC

A gap analysis: what stands between this backend and being a drop-in
ɴsɪ renderer for Houdini or Gaffer.

Read against the specification's node reference, this crate's source,
and the three real ɴsɪ client implementations -- Gaffer's
`IECoreDelight`, 3Delight for Houdini, and `hdNSI`. Where a claim is
about what a DCC does, it is from that client's source rather than
from its documentation.

**The dispatch that decides most of this is one match**, in
`src/flush.rs`. A node type absent from it is skipped and reported; an
attribute absent from the arm that handles its node was dropped in
silence, and that second half was the dangerous one.

## Status

**Written as a gap analysis, kept as a record.** The sweep this
document asked for was built, and each blocker below now carries what
became of it.

Twelve are closed. Three are closed as far as this repository can close
them and are now *loud* rather than silent, which was the point:
`NSIEvaluate` needs a `Scene::merge` upstream (7); interactive delivery
is one RGBA layer from one driver and says what does not arrive (12);
a multi-camera scene still picks the last chain and names the ones it
dropped (16). One is written and untested against a real host: the
display driver (1) is exported and wired end to end here, but no Gaffer
or Houdini viewport has been driven through it.

## The pattern worth calling out first

The stated contract is that nothing refuses a scene and what cannot be
carried is recorded in `Flushed::limitations` (`flush.rs:9-16`). That
was honoured scrupulously for everything the flush *looked at*.

The dangerous class was what it never looked at: `visibility.*`,
`matte`, `nholes`, `environment.angle`, the whole `.global` node, and
every `screen` and `outputlayer` attribute beyond the handful mapped.
Those produce a plausible image of the wrong scene with nothing saying
so -- exactly the failure this repository's design notes exist to
prevent.

**That class is precisely where both DCCs live.** Gaffer's eight
per-object attributes, its `dl:` globals and its extra `screen` and
`outputlayer` attributes were all set, all ignored, and none reported.

**Done.** `report_unread` sweeps every attribute set on a node type the
flush handles and names what it never reads, per node, against a
`CONSUMED` table of what each arm consumes. `report_unread_global` does
the same for `.global`.

The sweep has a failure mode of its own, met twice and worth recording:
a report that says the opposite of the truth is worse than no report,
because the loud reports are only worth having if they can be believed.
An OSL shader's parameters cross in the group specification whatever
they are called, and a light driven by an `OslMap` has not lost its
photometry. Both were being reported as dropped, and both are now
exempt with a test pinning each direction.

## The sixteen blockers, and what became of them

Ranked as first written. Each entry says what it was and what closed
it.

1. ~~**No display driver.**~~ **Written, untested against a host.**
   `DspyRegisterDriver` and `DspyRegisterDriverTable` are exported and
   feed a registry; `outputdriver.drivername` selects from it, and a
   name nothing registered is reported along with the names that were.
   The interface leaves the driver API unspecified and everyone uses
   RenderMan's Dspy, so this is the shape both DCCs expect. What has
   not happened is a Gaffer or Houdini viewport actually receiving a
   pixel through it, and that needs one of them rather than more code.

2. ~~**`NSIBegin` discards `errorhandler`.**~~ **Done.** The handler is
   captured at `NSIBegin` and every diagnostic goes through it,
   stderr only when none was given. This gated the usefulness of
   everything else: a user could not see *why* a render was wrong.

3. ~~**The `attributes` node is unread.**~~ **Done, one residual.**
   `visibility.*` maps onto MoonRay's nine flags, for meshes, curves,
   particles and volumes alike -- the mesh was the only kind that got
   them until this pass. `matte` is reported, MoonRay having no
   holdout. `bounds` is still unread and is named by the sweep.

4. ~~**`NSIConnect` discards its arguments.**~~ **Done.** This was the
   one that corrupted rather than limited: `index` decides which
   prototype a multi-prototype instancer places, so through the C API
   it placed the wrong models in silence. `connect_with_arguments`
   carries the whole argument list, so `index` pairs and `priority`
   breaks ties.

5. ~~**`curves` and `particles` are unsupported.**~~ **Done.** Hair,
   fur, grass and point clouds cross as `RdlCurveGeometry` and
   `RdlPointGeometry`.

6. ~~**Six emitter names become lights.**~~ **Done, and then some.**
   Every ɴsɪ emitter becomes a `MeshLight` whose radiance is an
   `OslMap` running the network itself, so the light's colour, its
   intensity and how both vary across its surface are the closure's.
   The name table survives only as the fallback for a build with no
   OSL, and says so when it is used.

7. **`NSIEvaluate` does nothing -- but says so.** It parses `type`,
   `filename` and `script` and reports that everything the call would
   have created is missing from the render rather than wrong in it.
   Running it needs a `Scene::merge` upstream, so this is the one
   blocker whose fix is not in this repository.

8. ~~**`.global` is entirely unread.**~~ **Done.** Nine names reach
   `SceneVariables` -- shading samples, the five ray depths, texture
   memory and the thread count -- and the other thirty-odd are named
   rather than dropped. Gaffer forwards its whole quality interface
   here.

9. ~~**`lightdepth "auto"` is unhandled.**~~ **Done.** The
   `.direct`/`.indirect` suffix is stripped before the lobe is
   matched, and applied as the constraint on the path. Both DCCs name
   layers this way and relied on it.

10. ~~**`id.*` builtin AOVs are unmapped.**~~ **Done.** They become a
    Cryptomatte output, with a limitation saying MoonRay has one rather
    than an identity per kind.

11. ~~**No light linking, shadow linking or light groups.**~~ **Done.**
    ɴsɪ has no light-linking attribute -- §4.5 defers to §4.8's
    inter-object visibility, a cross-hierarchy connection into another
    object's `visibility` carrying a `"value"`. Three shapes of it map,
    and which column they land in is decided by the ray type and by
    which end is a light: geometry into a light's `visibility` is a
    per-row `LightSet` built by subtraction; a light into a shape's
    `visibility.shadow` is a `ShadowSet`; shape into shape is a
    `ShadowReceiverSet`. The last two collect what is excluded rather
    than what is kept, because that is the polarity
    `ShadowLinking::canCastShadow` reads. Light groups were already
    done, as light-path expression labels. General per-pair visibility
    has no MoonRay counterpart and is reported by pair.

12. **Interactive delivery is one RGBA layer from one driver**, and
    `stoppedcallback` is still not called. Neither is silent now: a
    second driver carrying callbacks, a scene naming more than one
    output layer, and a registered `stoppedcallback` each get a line
    saying what will not arrive and what to do instead.

13. ~~**`faceset` is unsupported.**~~ **Done.** A face set becomes a
    MoonRay part, and one with its own shader becomes a second layer
    row -- which is what the interface means by attaching attributes to
    some faces.

14. ~~**No environment texture.**~~ **Done.** The shader's texture
    parameter reaches `EnvLight.texture`, and an unrecognised spelling
    is reported by name. `environment.angle` is genuinely unmappable:
    `EnvLight` has no cone, and `sample_upper_hemisphere_only`'s axis
    is the light's rather than the cone's, so mapping 180 onto it would
    be right only by coincidence. Reported in the environment's own
    words, because a sun rendering as a full sky is a plausible frame
    and the difference is the whole look.

15. ~~**No depth of field.**~~ **Done**, bokeh included. Lens shift has
    no counterpart on a perspective camera, whose framing follows `fov`
    alone; it is reported rather than approximated, and an orthographic
    camera does carry its window. `shutteropening` is unread and named
    by the sweep: MoonRay has a shutter *bias*, which is a skew rather
    than a trapezoid.

16. ~~**Multi-camera scenes *silently* pick the wrong camera.**~~
    **Half done.** MoonRay renders one camera per frame and the
    interface does not promise that, so the last chain still wins --
    but the report names the camera used and the ones dropped, and the
    resolution no longer comes from a different screen than the camera.

## What is fully carried

`transform`, `root` and `instances`. The last is the strongest part of
the backend: prototypes, per-instance matrices, model indices and
disabled instances all cross natively and nest five deep. Its one loss
-- a moving instancer blurs translation only, because rdl2's
`xform_list` is not blurrable -- is reported.

## What is partly carried

Nothing on this list is silent any more. What a node drops, it names.

- **`mesh`** carries positions with motion, indices, winding,
  subdivision scheme, creases, corners, `st`, `N`, face sets,
  visibility and arbitrary primitive variables in all four
  interpolations. `nholes` and `quadraticmotion` are dropped, so a mesh
  with holes renders them solid and three-sample curved blur becomes
  linear.
- **The cameras** carry transform, clipping range, field of view, the
  fisheye mapping and depth of field with bokeh. Lens shift on a
  perspective camera, shutter shape and the lens shader connection have
  no counterpart.
- **`screen`** carries resolution and the screen window.
- **`outputlayer`** carries the name, the variable and its source, the
  scalar format, the light set as a path-expression label and the light
  depth. A layer fanning out to several drivers still loses all but the
  first.
- **`environment`** becomes an `EnvLight` with its transform, its
  texture and its intensity. Its cone angle has no counterpart.
- **`shader`** is the strongest part with OSL running: the network
  crosses as a group specification, validated against the compiled
  shader first, with three classes of mistake reported by handle. A
  shader that both emits and shades is the one case with no faithful
  mapping, and the surface is kept.
- **`volume`** carries the file and its grids, and a bound
  `volumeshader` runs as OSL.

## What is not carried at all

`plane`, `procedural`, `vdbparticles`, `cylindricalcamera`, `nurbs`,
`t-nurcc`.

All are reported rather than dropped -- and only these. A node type the
flush consumes somewhere other than its dispatch, such as a `set`
flattened into a light set, used to reach the same catch-all and come
back as "no MoonRay mapping" while its members were being carried.
Four of the six are genuine
renderer gaps rather than unimplemented mapping: `vdbparticles` needs
an OpenVDB point-grid reader MoonRay does not have,
`cylindricalcamera` has no MoonRay projection, and `nurbs` and
`t-nurcc` are draft in the specification and implemented by nobody.

## Two that are correct as they stand

`suspend` and `resume` are deliberately unmapped,
and Gaffer's own source says 3Delight does not support them either.
The interactive sequence every real client uses -- `start` with
`interactive`, `synchronize` per edit, `stop` -- is exactly what this
backend implements.
