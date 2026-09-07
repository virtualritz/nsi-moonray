# Probing ɴsɪ, With ɴsɪ's Own Renderer

`tools/oracle` reads MoonRay. This reads the other side: what ɴsɪ
*means*. Four tasks were parked on questions no header here could
answer -- which axis `fov` names, whether a mesh carries velocity, what
a shader's parameters are called, how an area light is recognised --
and the honest answer to each was "whoever has the specification can
finish this in minutes".

3Delight is that specification's reference implementation and its free
build ships everything needed: `doc/nsi.pdf`, `renderdl`, `oslc`, and
178 compiled ɴsɪ shaders. So the questions become measurements, and
the findings are written up as `F11` in
`specs/001-moonray-backend/research.md`.

## Running

Download 3Delight, unpack it anywhere, and point at it:

```bash
DELIGHT=/path/to/3delight/Linux-x86_64
export PATH=$DELIGHT/bin:$PATH
export LD_LIBRARY_PATH=$DELIGHT/lib:$LD_LIBRARY_PATH
export DL_SHADERS_PATH=.:$DELIGHT/osl

oslc -I$DELIGHT/osl tools/probe/emitter.osl   # into the working directory
renderdl tools/probe/framing.nsi
python3 tools/probe/lit.py framing.png

tools/probe/motion.sh /tmp/motion
tools/probe/parameters.sh $DELIGHT/osl
```

`renderdl --version` should report a free build; no licence is needed.

## The probes

| Probe | Question | What it measures |
| --- | --- | --- |
| `framing.nsi` | is `fov` vertical or horizontal? | a quad that fills exactly one axis, on a 2:1 frame so the two cannot be confused |
| `motion.sh` | does a mesh carry velocity? | the same quad seven times, once per candidate attribute name, against two position samples as the control |
| `parameters.sh` | what are a shader's parameters called? | `^param` lines out of the shipped `.oso` files |

`lit.py` reports the lit rectangle in a PNG using the standard library
only, so a bounding box costs no dependency.

`emitter.osl` is the specification's own listing 4.2, reduced to what
the probes need: a surface visible without a light, so what is measured
is geometry and framing rather than shading.

## What they said

- **`fov` is vertical.** The quad filled all 200 rows and 200 of 400
  columns. `flush::focal()` already read it that way, borrowed from how
  `nsi_toolbelt` uses it; it is now confirmed, and
  `inprocess::the_frame_matches_3delights_framing` renders the same
  probe through MoonRay and asserts the same rectangle.
- **A mesh carries no velocity.** Two position samples smear;
  `velocity`, `v`, `V`, `vel` and `motion` are all ignored in silence.
  ɴsɪ has velocity only on the two OpenVDB nodes, as a grid name.
- **Shader parameters belong to the shader**, and the shaders in
  practical use are a table -- `flush.rs`'s `PARAMETERS`, read off
  these files rather than guessed.
- **A light is a shader too.** ɴsɪ has no light nodes; geometry whose
  shader emits *is* the light. `parameters.sh areaLight pointLight
  spotLight distantLight` reads the other half of that table --
  `flush.rs`'s `LIGHTS` -- and shows the three parameters every one of
  them shares with `scene_rdl2`'s `Light` base class.
