//! The ɴsɪ C entry points, so MoonRay can be loaded where 3Delight is.
//!
//! `nsi-ffi-wrap` reaches a renderer by `dlopen`ing a library and
//! looking up a fixed set of `NSI*` symbols — the library's name and the
//! environment variable that finds it are parameters of its
//! `define_nsi_renderer!` macro, not constants. So a `cdylib` exporting
//! those symbols *is* a renderer as far as any ɴsɪ consumer is
//! concerned, and one that records the scene and hands it to MoonRay is
//! a drop-in replacement for `lib3delight.so`.
//!
//! ```text
//! MOONRAY_NSI=…/libnsi_moonray.so   your application
//!                                          │  NSICreate, NSIConnect, …
//!                                          ▼
//!                                   nsi_intermediate::Scene
//!                                          │  flush
//!                                          ▼
//!                                       .rdla ──▶ moonray -in … -out …
//! ```
//!
//! # What this is not, yet
//!
//! A batch renderer behind an interactive interface. `NSIRenderControl`
//! `"start"` writes the scene and runs MoonRay to completion, so
//! `"synchronize"`, `"suspend"` and `"resume"` have nothing to act on
//! and pixels come back as a file rather than through a display driver.
//! Reaching MoonRay's progressive modes means linking `libmoonray`
//! instead of spawning it; see `specs/001-moonray-backend/tasks.md`
//! `T4.4`.
//!
//! # Safety
//!
//! Every function here is called from C with raw pointers. Each is
//! `unsafe` by nature and defensive by construction: a null handle, a
//! null parameter array or an unknown context is ignored rather than
//! dereferenced, because ɴsɪ's contract is that a renderer keeps going
//! and reports, and because a crash inside a host application is the
//! worst possible failure mode.

use crate::{
    display::{self, Callbacks},
    flush::flush,
    render::Render,
};
use nsi_intermediate::{HostPtr, OwnedArg, OwnedData, Scene};
use nsi_trait::{FfiParam, Type};
use std::{
    collections::HashMap,
    ffi::{CStr, c_char, c_int, c_void},
    path::PathBuf,
    sync::{LazyLock, Mutex},
};

/// `NSIContext_t`, which the C API defines as `int` with 0 reserved for
/// a bad context.
pub type NsiContext = c_int;

/// Everything one ɴsɪ context holds.
struct Context {
    scene: Scene,
    /// Where the `.rdla` was written, kept so a failed render can say
    /// what to look at.
    scene_file: Option<PathBuf>,
}

static CONTEXTS: LazyLock<Mutex<HashMap<NsiContext, Context>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The next context id. Never 0: that is `NSI_BAD_CONTEXT`.
static NEXT: Mutex<NsiContext> = Mutex::new(1);

/// Run `body` with one context's scene, or do nothing if the context is
/// not one of ours. An unknown context is a caller error, and ignoring
/// it is what keeps a host application alive.
fn with<R>(ctx: NsiContext, body: impl FnOnce(&mut Context) -> R) -> Option<R> {
    let mut contexts = CONTEXTS.lock().ok()?;
    contexts.get_mut(&ctx).map(body)
}

/// The bytes of a C string, empty when it is null.
///
/// # Safety
///
/// `pointer` is null or points at a NUL-terminated string.
unsafe fn bytes(pointer: *const c_char) -> Vec<u8> {
    if pointer.is_null() {
        return Vec::new();
    }

    // SAFETY: the caller guarantees a NUL-terminated string.
    unsafe { CStr::from_ptr(pointer) }.to_bytes().to_vec()
}

/// A borrowed C string, or `None` when it is null.
///
/// # Safety
///
/// `pointer` is null or points at a NUL-terminated string.
unsafe fn string(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }

    // SAFETY: the caller guarantees a NUL-terminated string.
    Some(
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned(),
    )
}

/// Copy a C parameter array into owned arguments.
///
/// This mirrors `OwnedArg::from_param`, which cannot be reused here: it
/// takes a `nsi-ffi-wrap` `Arg`, and what arrives across the C boundary
/// is the raw struct. The one subtlety is the element count — the C
/// `count` field counts *elements*, and an `array_length`-ed parameter
/// holds `count * array_length` of them. Reading `count` alone would
/// silently truncate every array-typed attribute.
///
/// # Safety
///
/// `params` points at `count` valid `FfiParam`s, each describing data
/// of the type it names.
unsafe fn arguments(params: *const FfiParam, count: c_int) -> Vec<OwnedArg> {
    if params.is_null() || count <= 0 {
        return Vec::new();
    }

    // SAFETY: the caller guarantees `count` valid parameters.
    let params = unsafe { std::slice::from_raw_parts(params, count as usize) };

    params
        .iter()
        .filter_map(|param| unsafe { argument(param) })
        .collect()
}

/// # Safety
///
/// `param` describes valid data of the type it names.
unsafe fn argument(param: &FfiParam) -> Option<OwnedArg> {
    let name = unsafe { string(param.name) }?;
    let type_tag = tag(param.type_)?;

    let array_length = param.arraylength.max(1) as usize;
    let elements = param.count * array_length;
    let scalars = elements * components(type_tag);

    if param.data.is_null() {
        return None;
    }

    // SAFETY: `data` points at `scalars` values of `type_tag`, by the
    // ɴsɪ parameter contract the caller is bound by.
    let data = unsafe {
        match type_tag {
            Type::F32
            | Type::Color
            | Type::Point
            | Type::Vector
            | Type::Normal
            | Type::MatrixF32 => OwnedData::F32(
                std::slice::from_raw_parts(param.data as *const f32, scalars)
                    .to_vec(),
            ),
            Type::F64 | Type::MatrixF64 => OwnedData::F64(
                std::slice::from_raw_parts(param.data as *const f64, scalars)
                    .to_vec(),
            ),
            Type::I32 => OwnedData::I32(
                std::slice::from_raw_parts(param.data as *const i32, scalars)
                    .to_vec(),
            ),
            Type::I64 => OwnedData::I64(
                std::slice::from_raw_parts(param.data as *const i64, scalars)
                    .to_vec(),
            ),
            // Bytes, not `String`: an ɴsɪ string is whatever the C
            // API was handed, and a file name is not required to be
            // UTF-8. Repairing it here would lose the original at
            // recording time, where no later care could undo it.
            Type::String => OwnedData::String(
                std::slice::from_raw_parts(
                    param.data as *const *const c_char,
                    scalars,
                )
                .iter()
                .map(|pointer| bytes(*pointer))
                .collect(),
            ),
            // `Reference` never reaches MoonRay (`spec.md` R2) and
            // must still survive recording: an application's
            // output-driver callbacks arrive this way, and dropping
            // them leaves a viewport with a driver and nothing to
            // call. ɴsɪ does not copy a `Reference` -- the pointee
            // belongs to the caller, who keeps it alive -- so what is
            // stored is the pointer itself.
            Type::Reference => OwnedData::Reference(
                std::slice::from_raw_parts(
                    param.data as *const *const c_void,
                    scalars,
                )
                .iter()
                .map(|pointer| HostPtr(*pointer))
                .collect(),
            ),
            Type::Invalid => return None,
        }
    };

    Some(OwnedArg::new(
        name,
        type_tag,
        array_length,
        param.flags,
        data,
    ))
}

/// The `NSIType_t` value as a type, or `None` if it is not one.
fn tag(value: c_int) -> Option<Type> {
    Some(match value {
        1 => Type::F32,
        0x11 => Type::F64,
        2 => Type::I32,
        0x12 => Type::I64,
        3 => Type::String,
        4 => Type::Color,
        5 => Type::Point,
        6 => Type::Vector,
        7 => Type::Normal,
        8 => Type::MatrixF32,
        0x18 => Type::MatrixF64,
        9 => Type::Reference,
        _ => return None,
    })
}

/// Scalars per element, as `nsi-intermediate` counts them.
const fn components(type_tag: Type) -> usize {
    match type_tag {
        Type::Color | Type::Point | Type::Vector | Type::Normal => 3,
        Type::MatrixF32 | Type::MatrixF64 => 16,
        _ => 1,
    }
}

/// Look up a string argument by name.
fn argument_string(arguments: &[OwnedArg], name: &str) -> Option<String> {
    arguments
        .iter()
        .find(|argument| argument.name == name)
        .and_then(|argument| match &argument.data {
            OwnedData::String(values) => values
                .first()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        })
}

// ─── The C API ──────────────────────────────────────────────────────

/// # Safety
///
/// `params` points at `nparams` valid parameters, or is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn NSIBegin(
    _nparams: c_int,
    _params: *const FfiParam,
) -> NsiContext {
    let Ok(mut next) = NEXT.lock() else {
        return 0;
    };
    let ctx = *next;
    *next += 1;

    let Ok(mut contexts) = CONTEXTS.lock() else {
        return 0;
    };
    contexts.insert(
        ctx,
        Context {
            scene: Scene::default(),
            scene_file: None,
        },
    );

    ctx
}

#[unsafe(no_mangle)]
pub extern "C" fn NSIEnd(ctx: NsiContext) {
    if let Ok(mut contexts) = CONTEXTS.lock() {
        contexts.remove(&ctx);
    }
}

/// # Safety
///
/// `handle` and `type_` are NUL-terminated strings; `params` points at
/// `nparams` valid parameters, or is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn NSICreate(
    ctx: NsiContext,
    handle: *const c_char,
    type_: *const c_char,
    _nparams: c_int,
    _params: *const FfiParam,
) {
    let (Some(handle), Some(node_type)) =
        (unsafe { string(handle) }, unsafe { string(type_) })
    else {
        return;
    };

    with(ctx, |context| context.scene.create(&handle, &node_type));
}

/// # Safety
///
/// As [`NSICreate`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn NSIDelete(
    ctx: NsiContext,
    handle: *const c_char,
    _nparams: c_int,
    _params: *const FfiParam,
) {
    let Some(handle) = (unsafe { string(handle) }) else {
        return;
    };

    with(ctx, |context| context.scene.delete(&handle));
}

/// # Safety
///
/// As [`NSICreate`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn NSISetAttribute(
    ctx: NsiContext,
    object: *const c_char,
    nparams: c_int,
    params: *const FfiParam,
) {
    let Some(handle) = (unsafe { string(object) }) else {
        return;
    };
    let arguments = unsafe { arguments(params, nparams) };

    with(ctx, |context| {
        context.scene.set_attribute(&handle, arguments)
    });
}

/// # Safety
///
/// As [`NSICreate`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn NSISetAttributeAtTime(
    ctx: NsiContext,
    object: *const c_char,
    time: f64,
    nparams: c_int,
    params: *const FfiParam,
) {
    let Some(handle) = (unsafe { string(object) }) else {
        return;
    };
    let arguments = unsafe { arguments(params, nparams) };

    with(ctx, |context| {
        context
            .scene
            .set_attribute_at_time(&handle, time, arguments)
    });
}

/// # Safety
///
/// `object` and `name` are NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn NSIDeleteAttribute(
    ctx: NsiContext,
    object: *const c_char,
    name: *const c_char,
) {
    let (Some(handle), Some(name)) =
        (unsafe { string(object) }, unsafe { string(name) })
    else {
        return;
    };

    with(ctx, |context| {
        context.scene.delete_attribute(&handle, &name)
    });
}

/// # Safety
///
/// The four handle and attribute pointers are NUL-terminated strings or
/// null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn NSIConnect(
    ctx: NsiContext,
    from: *const c_char,
    from_attr: *const c_char,
    to: *const c_char,
    to_attr: *const c_char,
    _nparams: c_int,
    _params: *const FfiParam,
) {
    let (Some(from), Some(to), Some(to_attr)) =
        (unsafe { string(from) }, unsafe { string(to) }, unsafe {
            string(to_attr)
        })
    else {
        return;
    };
    // ɴsɪ leaves the source attribute empty for a whole-node
    // connection, and the C API spells that as either null or "".
    let from_attr = unsafe { string(from_attr) }.filter(|s| !s.is_empty());

    with(ctx, |context| {
        // An unmapped destination attribute is upstream's to reject;
        // here it means the connection is not recorded, which is what
        // `classify` refusing to guess is for.
        let _ =
            context
                .scene
                .connect(&from, from_attr.as_deref(), &to, &to_attr);
    });
}

/// # Safety
///
/// As [`NSIConnect`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn NSIDisconnect(
    ctx: NsiContext,
    from: *const c_char,
    from_attr: *const c_char,
    to: *const c_char,
    to_attr: *const c_char,
) {
    let (Some(from), Some(to), Some(to_attr)) =
        (unsafe { string(from) }, unsafe { string(to) }, unsafe {
            string(to_attr)
        })
    else {
        return;
    };
    let from_attr = unsafe { string(from_attr) }.filter(|s| !s.is_empty());

    with(ctx, |context| {
        let _ = context.scene.disconnect(
            &from,
            from_attr.as_deref(),
            &to,
            &to_attr,
        );
    });
}

/// # Safety
///
/// `params` points at `nparams` valid parameters, or is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn NSIEvaluate(
    _ctx: NsiContext,
    _nparams: c_int,
    _params: *const FfiParam,
) {
    // `NSIEvaluate` replays a `.nsi` stream or runs a procedural. Both
    // need a parser this crate does not have; see `T4.3`. Ignoring it
    // loses the stream's contents, which is why it is listed as a
    // limitation rather than quietly treated as a no-op success.
}

/// # Safety
///
/// `params` points at `nparams` valid parameters, or is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn NSIRenderControl(
    ctx: NsiContext,
    nparams: c_int,
    params: *const FfiParam,
) {
    let arguments = unsafe { arguments(params, nparams) };
    let action = argument_string(&arguments, "action").unwrap_or_default();

    // Only "start" does anything: the render is a batch, so by the time
    // it returns there is nothing left to wait for, synchronise or
    // suspend.
    if action != "start" {
        return;
    }

    with(ctx, |context| {
        let flushed = flush(&context.scene);

        for limitation in &flushed.limitations {
            eprintln!("nsi-moonray: {limitation}");
        }

        let path = scene_path(ctx);
        if let Err(error) = std::fs::write(&path, flushed.to_rdla()) {
            eprintln!("nsi-moonray: cannot write {}: {error}", path.display());
            return;
        }
        context.scene_file = Some(path.clone());

        // An application with a viewport hands its callbacks to the
        // `outputdriver` node and expects pixels back. Collect them
        // before the render, because the file each one wants is what
        // the render is told to write.
        let deliveries: Vec<(String, Callbacks, PathBuf)> = context
            .scene
            .nodes()
            .filter(|(_, node)| node.node_type == "outputdriver")
            .filter_map(|(handle, _)| {
                let callbacks = Callbacks::of(&context.scene, handle)?;
                let file = crate::flush::image_file(&context.scene, handle)?;
                Some((handle.clone(), callbacks, PathBuf::from(file)))
            })
            .collect();

        if let Err(error) = Render::new(&path).run() {
            // ɴsɪ always returns an image, and when it cannot, it says
            // so and leaves the scene where someone can look at it.
            eprintln!(
                "nsi-moonray: {error}; the scene is at {}",
                path.display()
            );
            return;
        }

        for (handle, callbacks, image) in deliveries {
            if let Err(error) = display::deliver_file(&callbacks, &handle, &image)
            {
                eprintln!("nsi-moonray: {handle:?} received no pixels: {error}");
            }
        }
    });
}

/// Where the `.rdla` for a context goes.
///
/// `$NSI_MOONRAY_SCENE` names it outright, which is how you get at the
/// scene a render was made from.
fn scene_path(ctx: NsiContext) -> PathBuf {
    match std::env::var_os("NSI_MOONRAY_SCENE") {
        Some(path) => PathBuf::from(path),
        None => std::env::temp_dir().join(format!("nsi-moonray-{ctx}.rdla")),
    }
}

/// The display-driver registration `nsi-ffi-wrap`'s `output` feature
/// looks up.
///
/// It has to exist or `dlopen` of this library fails outright for any
/// consumer built with that feature — the whole symbol table is
/// resolved up front. Batch rendering writes a file rather than feeding
/// a driver, so this records nothing yet; wiring drivers up is part of
/// linking `libmoonray` (`T4.4`).
///
/// # Safety
///
/// Called from C with a driver name and four function pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn DspyRegisterDriver(
    driver_name: *const c_char,
    _open: *const c_void,
    _write: *const c_void,
    _close: *const c_void,
    _query: *const c_void,
) -> c_int {
    let name = unsafe { string(driver_name) }.unwrap_or_default();
    eprintln!(
        "nsi-moonray: display driver {name:?} registered, but this build \
         renders in batch and writes a file; see T4.4"
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn c(text: &str) -> CString {
        CString::new(text).expect("no interior NUL in a test string")
    }

    /// The whole point: a scene built through the C entry points is the
    /// same scene the Rust API would have built.
    #[test]
    fn a_scene_arrives_through_the_c_api() {
        let ctx = unsafe { NSIBegin(0, std::ptr::null()) };
        assert_ne!(ctx, 0, "0 is NSI_BAD_CONTEXT");

        let handle = c("tri");
        let node_type = c("mesh");
        unsafe {
            NSICreate(
                ctx,
                handle.as_ptr(),
                node_type.as_ptr(),
                0,
                std::ptr::null(),
            )
        };

        let name = c("nvertices");
        let counts: [i32; 1] = [3];
        let param = FfiParam {
            name: name.as_ptr(),
            data: counts.as_ptr() as *const c_void,
            type_: Type::I32 as c_int,
            arraylength: 1,
            count: 1,
            flags: 0,
        };
        unsafe { NSISetAttribute(ctx, handle.as_ptr(), 1, &param) };

        let root = c(".root");
        let objects = c("objects");
        unsafe {
            NSIConnect(
                ctx,
                handle.as_ptr(),
                std::ptr::null(),
                root.as_ptr(),
                objects.as_ptr(),
                0,
                std::ptr::null(),
            )
        };

        let recorded = with(ctx, |context| {
            let node = context.scene.node("tri").expect("the node exists");
            (
                node.node_type.clone(),
                node.attrs.contains_key("nvertices"),
                context.scene.edges().count(),
            )
        })
        .expect("the context exists");

        assert_eq!(recorded, ("mesh".to_string(), true, 1));

        NSIEnd(ctx);
        assert!(with(ctx, |_| ()).is_none(), "the context is gone");
    }

    /// A `count` of 2 with an `arraylength` of 3 is six scalars, not
    /// two: reading `count` alone truncates every array attribute.
    #[test]
    fn an_array_parameter_is_read_whole() {
        let ctx = unsafe { NSIBegin(0, std::ptr::null()) };
        let handle = c("mesh");
        let node_type = c("mesh");
        unsafe {
            NSICreate(
                ctx,
                handle.as_ptr(),
                node_type.as_ptr(),
                0,
                std::ptr::null(),
            )
        };

        let name = c("P");
        let points: [f32; 6] = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let param = FfiParam {
            name: name.as_ptr(),
            data: points.as_ptr() as *const c_void,
            // Two points; `Point` is three scalars each.
            type_: Type::Point as c_int,
            arraylength: 1,
            count: 2,
            flags: 0,
        };
        unsafe { NSISetAttribute(ctx, handle.as_ptr(), 1, &param) };

        let scalars = with(ctx, |context| {
            match &context.scene.node("mesh").expect("the node exists").attrs["P"]
                .data
            {
                OwnedData::F32(values) => values.len(),
                _ => 0,
            }
        });

        assert_eq!(scalars, Some(6));
        NSIEnd(ctx);
    }

    /// Null pointers are ignored, not dereferenced. A host application
    /// dying inside a renderer is the worst failure this can have.
    #[test]
    fn null_arguments_are_survivable() {
        let ctx = unsafe { NSIBegin(0, std::ptr::null()) };
        unsafe {
            NSICreate(
                ctx,
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
            );
            NSISetAttribute(ctx, std::ptr::null(), 3, std::ptr::null());
            NSIDeleteAttribute(ctx, std::ptr::null(), std::ptr::null());
        }

        assert_eq!(with(ctx, |context| context.scene.nodes().count()), Some(0));
        NSIEnd(ctx);
    }

    /// An unknown context is ignored rather than panicking.
    #[test]
    fn an_unknown_context_does_nothing() {
        let handle = c("x");
        let node_type = c("mesh");
        unsafe {
            NSICreate(
                -1,
                handle.as_ptr(),
                node_type.as_ptr(),
                0,
                std::ptr::null(),
            )
        };
        NSIEnd(-1);
    }
}
