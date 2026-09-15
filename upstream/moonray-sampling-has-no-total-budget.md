<!--
Ready to file at https://github.com/OpenMoonRay/moonray/issues/new

Title: Sampling is controlled by separate per-lobe counts with no total
       budget, so a host cannot ask for "this much quality"

Not filed from here: this session's GitHub access is scoped to
`virtualritz`, and the MoonRay repository is on another tier.

Written against `scene_rdl2`'s `SceneVariables` as installed, read with
`rdl2_print --class SceneVariables`.
-->

# Sampling has no total budget, only per-lobe counts

## Summary

`SceneVariables` exposes sampling as a set of independent counts:

```
pixel_samples                 the square root of primary samples per pixel
bsdf_samples                  BSDF samples per shading point
light_samples                 light samples per shading point
bssrdf_samples
volume_illumination_samples
volume_indirect_samples
```

Each is set by hand, and nothing relates them. There is no attribute
that says "spend this much per pixel and decide the split yourself".

That is a reasonable interface for a TD tuning one shot. It is a poor
one for a **host application**, and it is the specific thing that makes
MoonRay hard to drive from a scene description that was designed the
other way round.

## Why this is a problem rather than a preference

ɴsɪ -- the interface this backend translates from -- deliberately
offers **one** number. `quality.shadingsamples` is documented as
"controls the quality of bsdf sampling", and that is the whole of it;
there is no separate light-sample count to set, because the
specification's position is that the renderer knows how to divide the
work and the artist does not want to.

That position is a reaction to something the industry already lived
through. RenderMan-family renderers accumulated on the order of two
dozen quality controls, and studios spent real days per show
discovering, by trial and error, which combination gave the most
quality per pixel for the least time. The knowledge was experiential,
went stale at every renderer release, and lived in the heads of a few
people. Nearly all of what it encoded is measurable by the renderer at
run time: which lobes are actually noisy, which lights actually matter
at this shading point, where the variance is. A renderer given a total
budget can make those calls per scene, or per pixel, and get it right
more often than a human can guess in advance.

So a host that wants to expose one quality slider -- which is what
artists ask for, and what ɴsɪ is built around -- has nowhere to put it.
It must either:

- pick a split itself and impose it on every scene, which is the guess
  the artist was trying not to make, only now made once by a programmer
  who has not seen the scene; or
- expose MoonRay's counts individually, which pushes the whole problem
  to the artist and abandons the interface's contract.

Neither is a translation. Both are a loss.

## What this backend does, and why it is a stopgap

`quality.shadingsamples` is forwarded to **both** `bsdf_samples` and
`light_samples`:

```rust
("quality.shadingsamples", "light_samples", ...),
("quality.shadingsamples", "bsdf_samples", ...),
```

Asking for eight and getting eight of one and one of the other is
plainly not what was asked, so both take the number. But that is a
guess too -- a fixed 1:1 split, applied to every scene, chosen because
there was nothing better to choose. A scene lit by one big area light
and a scene lit by forty small ones want very different splits, and
this gives them the same one.

`pixel_samples` is separate and that part is *correct*: ɴsɪ separates
anti-aliasing (`screen.oversampling`, camera rays per pixel) from
shading quality, exactly as MoonRay does. The gap is only in the
shading half, where MoonRay has several knobs and the interface has
one.

## What would close it

An attribute that names a budget rather than a split. Something of the
shape:

```
shading_samples      total shading samples per pixel; the renderer
                     divides them between BSDF, light, subsurface and
                     volume sampling according to measured variance
```

with the existing per-lobe counts kept as an override for anyone who
wants them -- so nothing is taken away, and a host gains a control it
can honestly map onto.

Adaptive sampling is already in the renderer (`min_adaptive_samples`,
`max_adaptive_samples`, `sampling_mode`), so the machinery for
"measure, then decide" exists. What is missing is that the same
thinking is not applied *across* lobes, only across pixels.

## Why it matters beyond this backend

Any host with a simple quality control hits this: a DCC's render
settings, a farm's quality preset, an interactive viewport trading
noise for latency. Each of them currently has to invent a split. They
will invent different ones, and a scene will look different depending
on which host submitted it -- which is the failure a shared scene
description exists to prevent.
