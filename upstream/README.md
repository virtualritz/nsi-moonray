# Upstream

Bugs found in MoonRay or `scene_rdl2` while building this backend,
written up for the projects that own them.

Each was found by *running* the code rather than reading it, and each
is here because a backend feeding MoonRay scenes built somewhere other
than MoonRay's own front end will keep finding this shape of thing: an
assumption that holds for a scene the command line assembled, and
crashes for one assembled in memory.

| Report | Status |
| --- | --- |
| [`moonray-empty-camera-crash.md`](moonray-empty-camera-crash.md) | Not filed — see the note at the top of the file |
| [`scene_rdl2-bvh-only-flag-ignored.md`](scene_rdl2-bvh-only-flag-ignored.md) | Not filed — as above |
| [`moonray-meshlight-needs-moonshine-dwa.md`](moonray-meshlight-needs-moonshine-dwa.md) | Not filed — as above |
| [`moonray-scalar-material-renders-black.md`](moonray-scalar-material-renders-black.md) | Not filed — as above |
| [`moonray-meshlight-map-shader-segfault.md`](moonray-meshlight-map-shader-segfault.md) | Not filed — as above |
| [`moonray-sampling-has-no-total-budget.md`](moonray-sampling-has-no-total-budget.md) | Not filed — as above. A control-surface gap rather than a defect: there is no way to ask for a quality budget and let the renderer divide it |
| [`moonray-shader-header-needs-tlos.md`](moonray-shader-header-needs-tlos.md) | Not filed — as above. A build defect: the installed `rdl2/Shader.h` needs a header MoonRay does not install, which clang catches and GCC does not |

The workaround for each lives in this repository and is marked as
such, so that fixing it upstream is a deletion rather than an
archaeology exercise.
