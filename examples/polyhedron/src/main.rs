//! A polyhedron, rendered by MoonRay through the ɴsɪ C API.
//!
//! Nothing here mentions MoonRay. `polyhedron-ops` sends the mesh to an
//! ɴsɪ context, `nsi-core` resolves a renderer at run time, and pointing
//! that resolution at `libnsi_moonray.so` is the whole trick. See the
//! README next to this file.

use nsi_core as nsi;
use polyhedron_ops::Polyhedron;

fn main() {
    let ctx = nsi::Context::new(None).expect("could not load an ɴsɪ renderer");

    let poly = Polyhedron::dodecahedron().kis(None, None, None, None, true).finalize();

    poly.to_nsi(&ctx, Some("poly"), None, None, None);
    ctx.connect("poly", None, ".root", "objects", None);

    // Pull the camera back: the polyhedron is centred on the origin, so
    // a camera left at the origin renders from inside it.
    ctx.create("cam_xform", nsi::node::TRANSFORM, None);
    ctx.set_attribute(
        "cam_xform",
        &[nsi::double_matrix!(
            "transformationmatrix",
            &[
                1., 0., 0., 0.,
                0., 1., 0., 0.,
                0., 0., 1., 0.,
                0., 0., 6., 1.,
            ]
        )],
    );
    ctx.connect("cam_xform", None, ".root", "objects", None);

    ctx.create("cam", nsi::node::PERSPECTIVE_CAMERA, None);
    ctx.set_attribute("cam", &[nsi::float!("fov", 35.)]);
    ctx.create("screen", nsi::node::SCREEN, None);
    ctx.set_attribute("screen", &[nsi::integers!("resolution", &[320, 240]).array_len(2)]);
    ctx.connect("screen", None, "cam", "screens", None);
    ctx.connect("cam", None, "cam_xform", "objects", None);

    ctx.create("beauty", nsi::node::OUTPUT_LAYER, None);
    ctx.set_attribute("beauty", &[nsi::string!("variablename", "Ci")]);
    ctx.connect("beauty", None, "screen", "outputlayers", None);
    ctx.create("driver", nsi::node::OUTPUT_DRIVER, None);
    ctx.set_attribute("driver", &[nsi::string!("imagefilename", "/tmp/poly.exr")]);
    ctx.connect("driver", None, "beauty", "outputdrivers", None);

    ctx.create("env", nsi::node::ENVIRONMENT, None);
    ctx.connect("env", None, ".root", "objects", None);

    ctx.render_control(nsi::Action::Start, None);
    ctx.render_control(nsi::Action::Wait, None);
}
