//! What a scene costs, in bytes and in seconds.
//!
//! Upstream interns handles behind `ustr_handles` and quotes its own
//! numbers. This measures the same thing on the shape *this* backend
//! sees: long hierarchical handles, two nodes per shape, two
//! connections each, and a flush at the end -- because the flush is
//! half the cost and upstream's numbers do not include it.
//!
//! ```bash
//! cargo run --release -- 50000                     # as it ships
//! cargo run --release --features interned -- 50000 # interned handles
//! ```
//!
//! Resident set rather than an allocator counter: it is what a farm
//! node runs out of, and it needs no allocator shim to read.
//! `research.md` F14 records what it said.

use nsi_intermediate::{OwnedArgument, OwnedData, Scene};
use nsi_trait::Type;

fn resident_kb() -> usize {
    let status = std::fs::read_to_string("/proc/self/status")
        .expect("this measures on Linux; /proc/self/status is how");

    status
        .lines()
        .find(|line| line.starts_with("VmRSS:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|kilobytes| kilobytes.parse().ok())
        .unwrap_or(0)
}

fn main() {
    let shapes: usize = std::env::args()
        .nth(1)
        .and_then(|count| count.parse().ok())
        .unwrap_or(50_000);

    let start = std::time::Instant::now();
    let before = resident_kb();

    let mut scene = Scene::default();
    scene.create("cam", "perspectivecamera").expect("recorded");
    scene
        .connect("cam", None, ".root", "objects")
        .expect("recorded");

    // Handles as a set-dressing scene actually spells them: long,
    // hierarchical, and repeated per shape. A benchmark using `"n0"`
    // would measure the map and not the strings.
    for shape in 0..shapes {
        let mesh = format!("/set/building_{shape:06}/wall/mesh");
        let transform = format!("/set/building_{shape:06}/wall/xform");

        scene.create(&mesh, "mesh").expect("recorded");
        scene.create(&transform, "transform").expect("recorded");
        scene
            .set_attribute(
                &mesh,
                vec![
                    OwnedArgument::new(
                        "nvertices",
                        Type::I32,
                        1,
                        0,
                        OwnedData::I32(vec![3]),
                    ),
                    OwnedArgument::new(
                        "P",
                        Type::Point,
                        3,
                        0,
                        OwnedData::F32(vec![
                            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                        ]),
                    ),
                ],
            )
            .expect("recorded");
        scene
            .connect(&mesh, None, &transform, "objects")
            .expect("recorded");
        scene
            .connect(&transform, None, ".root", "objects")
            .expect("recorded");
    }

    let recorded = resident_kb();
    let built = start.elapsed();

    let flushed = nsi_moonray::flush(&scene);
    let flushed_bytes = resident_kb();
    let total = start.elapsed();

    let nodes = shapes * 2 + 1;
    println!(
        "{shapes} shapes, {nodes} nodes, {} objects\n\
         scene   {:>7.1} MB  {:>5} B/node  {:>6.2}s\n\
         flush  +{:>7.1} MB  {:>5} B/object {:>6.2}s\n\
         total   {:>7.1} MB                 {:>6.2}s",
        flushed.document.objects.len(),
        (recorded - before) as f64 / 1024.0,
        (recorded - before) * 1024 / nodes,
        built.as_secs_f64(),
        (flushed_bytes - recorded) as f64 / 1024.0,
        (flushed_bytes - recorded) * 1024
            / flushed.document.objects.len().max(1),
        (total - built).as_secs_f64(),
        (flushed_bytes - before) as f64 / 1024.0,
        total.as_secs_f64(),
    );
}
