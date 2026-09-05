//! The built `cdylib`, loaded the way an ɴsɪ consumer loads a renderer.
//!
//! `nsi-ffi-wrap` `dlopen`s a library and resolves every `NSI*` symbol
//! up front — one missing symbol and the whole load fails, taking the
//! renderer with it. So this test opens the artefact this crate builds
//! and drives a scene through it by symbol, which is exactly what an
//! application does and what no unit test of the same functions can
//! show.

use libloading::{Library, Symbol};
use std::{
    ffi::{CString, c_char, c_int, c_void},
    path::PathBuf,
};

/// `NSIParam_t`.
#[repr(C)]
struct Param {
    name: *const c_char,
    data: *const c_void,
    type_: c_int,
    arraylength: c_int,
    count: usize,
    flags: c_int,
}

/// Where Cargo put the `cdylib`.
///
/// The test binary lives in `target/<profile>/deps`, so the library is
/// one directory up.
fn library() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_BIN_EXE_mrr"));
    path.pop();
    path.join(if cfg!(target_os = "macos") {
        "libnsi_moonray.dylib"
    } else if cfg!(windows) {
        "nsi_moonray.dll"
    } else {
        "libnsi_moonray.so"
    })
}

/// Every symbol `nsi-ffi-wrap` looks up, including the display-driver
/// registration its `output` feature needs. A consumer built with that
/// feature cannot load a library missing it, so its absence would be
/// invisible here and fatal there.
const SYMBOLS: [&[u8]; 12] = [
    b"NSIBegin",
    b"NSIEnd",
    b"NSICreate",
    b"NSIDelete",
    b"NSISetAttribute",
    b"NSISetAttributeAtTime",
    b"NSIDeleteAttribute",
    b"NSIConnect",
    b"NSIDisconnect",
    b"NSIEvaluate",
    b"NSIRenderControl",
    b"DspyRegisterDriver",
];

#[test]
fn every_symbol_the_loader_wants_resolves() {
    let path = library();
    let library = unsafe { Library::new(&path) }
        .unwrap_or_else(|error| panic!("loading {}: {error}", path.display()));

    for symbol in SYMBOLS {
        let found: Result<Symbol<'_, *const c_void>, _> =
            unsafe { library.get(symbol) };
        assert!(
            found.is_ok(),
            "{} does not export {}",
            path.display(),
            String::from_utf8_lossy(symbol)
        );
    }
}

/// A scene recorded through the loaded library, then written out by
/// asking it to render with no MoonRay present.
///
/// The render itself cannot happen here, and that is the point of the
/// assertion: it writes the `.rdla` first and reports the missing
/// renderer afterwards, so the scene survives a machine that cannot run
/// it.
#[test]
fn a_scene_recorded_through_the_library_is_written_out() {
    let path = library();
    let library = unsafe { Library::new(&path) }
        .unwrap_or_else(|error| panic!("loading {}: {error}", path.display()));

    type Begin = unsafe extern "C" fn(c_int, *const Param) -> c_int;
    type End = unsafe extern "C" fn(c_int);
    type Create = unsafe extern "C" fn(
        c_int,
        *const c_char,
        *const c_char,
        c_int,
        *const Param,
    );
    type SetAttribute =
        unsafe extern "C" fn(c_int, *const c_char, c_int, *const Param);
    type Connect = unsafe extern "C" fn(
        c_int,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        c_int,
        *const Param,
    );
    type RenderControl = unsafe extern "C" fn(c_int, c_int, *const Param);

    let scene_file = std::env::temp_dir().join("nsi-moonray-dropin.rdla");
    let _ = std::fs::remove_file(&scene_file);
    // SAFETY: single-threaded test setup, before any thread reads it.
    unsafe { std::env::set_var("NSI_MOONRAY_SCENE", &scene_file) };

    unsafe {
        let begin: Symbol<'_, Begin> = library.get(b"NSIBegin").unwrap();
        let create: Symbol<'_, Create> = library.get(b"NSICreate").unwrap();
        let set: Symbol<'_, SetAttribute> =
            library.get(b"NSISetAttribute").unwrap();
        let connect: Symbol<'_, Connect> = library.get(b"NSIConnect").unwrap();
        let control: Symbol<'_, RenderControl> =
            library.get(b"NSIRenderControl").unwrap();
        let end: Symbol<'_, End> = library.get(b"NSIEnd").unwrap();

        let ctx = begin(0, std::ptr::null());
        assert_ne!(ctx, 0, "0 is NSI_BAD_CONTEXT");

        let mesh = CString::new("tri").unwrap();
        let mesh_type = CString::new("mesh").unwrap();
        create(ctx, mesh.as_ptr(), mesh_type.as_ptr(), 0, std::ptr::null());

        let nvertices = CString::new("nvertices").unwrap();
        let counts: [i32; 1] = [3];
        let indices_name = CString::new("P.indices").unwrap();
        let indices: [i32; 3] = [0, 1, 2];
        let points_name = CString::new("P").unwrap();
        let points: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

        let params = [
            Param {
                name: nvertices.as_ptr(),
                data: counts.as_ptr() as *const c_void,
                type_: 2, // I32
                arraylength: 1,
                count: 1,
                flags: 0,
            },
            Param {
                name: indices_name.as_ptr(),
                data: indices.as_ptr() as *const c_void,
                type_: 2,
                arraylength: 1,
                count: 3,
                flags: 0,
            },
            Param {
                name: points_name.as_ptr(),
                data: points.as_ptr() as *const c_void,
                type_: 5, // Point
                arraylength: 1,
                count: 3,
                flags: 0,
            },
        ];
        set(ctx, mesh.as_ptr(), params.len() as c_int, params.as_ptr());

        let root = CString::new(".root").unwrap();
        let objects = CString::new("objects").unwrap();
        connect(
            ctx,
            mesh.as_ptr(),
            std::ptr::null(),
            root.as_ptr(),
            objects.as_ptr(),
            0,
            std::ptr::null(),
        );

        let action = CString::new("action").unwrap();
        let start = CString::new("start").unwrap();
        let start_pointer = start.as_ptr();
        let control_params = [Param {
            name: action.as_ptr(),
            data: &start_pointer as *const *const c_char as *const c_void,
            type_: 3, // String
            arraylength: 1,
            count: 1,
            flags: 0,
        }];
        control(ctx, 1, control_params.as_ptr());

        end(ctx);
    }

    let written =
        std::fs::read_to_string(&scene_file).unwrap_or_else(|error| {
            panic!("reading {}: {error}", scene_file.display())
        });

    assert!(written.contains("RdlMeshGeometry(\"tri\") {"), "{written}");
    assert!(
        written.contains(
            "[\"vertex_list_0\"] = { Vec3(0, 0, 0), Vec3(1, 0, 0), \
             Vec3(0, 1, 0)},"
        ),
        "{written}"
    );

    let _ = std::fs::remove_file(&scene_file);
}
