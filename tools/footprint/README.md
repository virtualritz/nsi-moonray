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

| | scene | per node | record | flush | per object | total |
| --- | --- | --- | --- | --- | --- | --- |
| as it ships | 144.7 MB | 1517 B | 32.6 s | +100.1 MB | 2098 B | 244.8 MB |
| `interned` | 103.1 MB | 1080 B | 11.7 s | +71.2 MB | 1493 B | 174.3 MB |

**29 % off the scene, 29 % off the document, and 2.8× faster to
record.** The speed is not a side effect of the size:
`edges_to_attribute` had to build two `String`s to probe its key on
every call, and interned it probes with a pair of `u64`s. Flushing
takes half a second either way, so all of that 21 seconds was scene
recording.

`nsi-moonray`'s `interned_handles` feature turns on both halves —
upstream's `ustr_handles` and this crate's interned
[`Name`](../../src/name.rs) — and is **on by default** on the strength
of these numbers.

This is also the instrument that found the document problem it just
measured the fix for. Before `Name`, the flushed document cost 109 MB
against the scene's 103 MB — *more than the scene it came from*, and
unmoved by upstream's interning, because `Document` copied every handle
into `String`s of its own. It is now 71 MB. `research.md` F14.
