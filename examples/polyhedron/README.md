# Polyhedron Demo

Builds a polyhedron with
[`polyhedron-ops`](https://github.com/virtualritz/polyhedron-ops), hands
it to the ɴsɪ context that crate already knows how to talk to, and
renders it with MoonRay.

Nothing in this program mentions MoonRay. It makes ordinary ɴsɪ calls
through `nsi-core`, which resolves a renderer at run time — so pointing
that resolution at `libnsi_moonray.so` is the whole trick, and the
demonstration is that neither `polyhedron-ops` nor `nsi-core` needs to
know anything changed.

```bash
cargo build                       # in the repository root: libnsi_moonray.so

# `nsi-core` looks for `$DELIGHT/lib/lib3delight.so`, so give it one.
mkdir -p /tmp/nsi-moonray/lib
ln -sf "$PWD/target/debug/libnsi_moonray.so" \
       /tmp/nsi-moonray/lib/lib3delight.so

DELIGHT=/tmp/nsi-moonray \
MOONRAY_ROOT=/path/to/moonray/install \
NSI_MOONRAY_SCENE=/tmp/poly.rdla \
    cargo run --manifest-path examples/polyhedron/Cargo.toml
```

`$NSI_MOONRAY_SCENE` is where the `.rdla` this backend wrote is left, so
the scene behind a render can be read.

## Its Own Manifest

This is not a `cargo` example of the crate, and it is not in the
workspace, because `polyhedron-ops` declares

```toml
[target.'cfg(target_os = "linux")'.dependencies.bevy]
version = "*"
```

— not optional. Depending on it from the backend would make every
`cargo test` here build bevy, winit and a wayland stack. Kept separate,
that cost falls only on whoever runs the demo.
