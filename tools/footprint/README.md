# What A Scene Costs

Upstream interns ɴsɪ handles behind `ustr_handles` and quotes its own
numbers. This measures the same thing on the shape *this* backend sees:
long hierarchical handles, two nodes per shape, two connections each,
and a flush at the end — because the flush is half the memory and
upstream's numbers do not include it.

```bash
cargo run --release -- 50000                      # as upstream ships
cargo run --release --features interned -- 50000  # interned handles
```

Resident set rather than an allocator counter: it is what a farm node
runs out of, and it needs no allocator shim to read. Linux only, for
the same reason — `/proc/self/status` is where the number is.

Its own workspace, deliberately: it has to be buildable with a feature
the crate under measurement does not have on, which is what makes the
two halves differ in exactly one thing.

## What it said

50 000 shapes — 100 001 nodes, 50 005 rdl2 objects — on this container:

| | scene | per node | build | flush | per object |
| --- | --- | --- | --- | --- | --- |
| as upstream ships | 144.6 MB | 1516 B | 35.3 s | +109.8 MB | 2303 B |
| `interned` | 103.1 MB | 1081 B | 11.3 s | +109.1 MB | 2288 B |

**29 % smaller and 3.1× faster to build.** The speed is not a side
effect of the size: `edges_to_attribute` had to build two `String`s to
probe its key on every call, and interned it probes with a pair of
`u64`s. Flushing takes half a second either way, so all of that 24
seconds was scene recording.

`nsi-moonray`'s `interned_handles` feature forwards to upstream's, and
is **on by default** on the strength of these numbers.

The other thing this says is about **this** crate: the flushed document
is now larger than the scene it came from, 109 MB against 103 MB, at
2288 bytes an object. `Document` copies every handle into `String`s —
`Object::name`, and `Reference` twice per `Layer` row — and hands back
much of what upstream just saved. `research.md` F14.
