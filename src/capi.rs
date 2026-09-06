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
//!                                       Document
//!                        apply │                 │ to_rdla
//!                              ▼                 ▼
//!                   MoonRay, linked          .rdla, a *dump*
//!                              │                 │
//!                     snapshot │                 └─▶ moonray -in …
//!                              ▼                     (the fallback)
//!                       callback.write
//! ```
//!
//! # Which path a scene takes
//!
//! With the `rdl2` feature and MoonRay linked, `NSIRenderControl`
//! `"start"` renders **in this process**: the scene goes into the
//! renderer's own `SceneContext`, the frame converges, and each
//! snapshot reaches the application's `outputdriver` callbacks. No file
//! is written and no process is spawned.
//!
//! It falls back to spawning the `moonray` binary when there is no
//! renderer to use -- MoonRay's `rdl2dso` not found, or a render
//! already running, since MoonRay's driver state is global and allows
//! one at a time. ɴsɪ always returns an image, so an application that
//! cannot have the fast path still gets its render.
//!
//! # What this is not, yet
//!
//! Interactive. The frame is rendered to completion, so
//! `"synchronize"`, `"suspend"` and `"resume"` still have nothing to
//! act on -- applying an *edit* to a live scene is
//! `specs/002-interactive-updates` `I1`-`I6`. The machinery it needs
//! now exists: a live `SceneContext` to edit and a `RenderContext` to
//! restart.
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
            // Bytes, not `String`. The spec says an ɴsɪ string is
            // UTF-8 -- 3Delight has agreed to write that down -- but
            // the C API is handed a `const char*`, and a caller that
            // hands over something else is not stopped by a promise.
            // Recording the bytes keeps that a reporting problem
            // later rather than a panic at the boundary.
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

    // Only "start" does anything on the spawned path: the render is a
    // batch, so by the time it returns there is nothing to wait for,
    // synchronise or suspend. The linked path answers more of these --
    // see `render_in_process`.
    if action != "start" {
        return;
    }

    #[cfg(all(feature = "rdl2", moonray))]
    {
        // The linked renderer is the path; spawning is what happens
        // when MoonRay is not where this expects it.
        let rendered = with(ctx, |context| render_in_process(&context.scene))
            .unwrap_or(false);
        if rendered {
            return;
        }
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
            if let Err(error) =
                display::deliver_file(&callbacks, &handle, &image)
            {
                eprintln!(
                    "nsi-moonray: {handle:?} received no pixels: {error}"
                );
            }
        }
    });
}

/// Render this context in this process, if MoonRay can be reached.
///
/// The path an application actually wants: the scene goes straight into
/// the renderer's own `SceneContext`, the frame converges, and each
/// snapshot reaches the application's `outputdriver` callbacks. No file
/// is written and no process is spawned.
///
/// Returns `false` when there is no renderer to use -- no `rdl2dso` to
/// point at, or one already live in this process -- and the caller
/// falls back to spawning. ɴsɪ always returns an image: an application
/// that cannot have the fast path should still get its render.
#[cfg(all(feature = "rdl2", moonray))]
pub fn render_in_process(scene: &nsi_intermediate::Scene) -> bool {
    let Some(dso) = moonray_dso_path() else {
        eprintln!(
            "nsi-moonray: $NSI_MOONRAY_DSO or $MOONRAY_ROOT names \
             MoonRay's `rdl2dso`; without it the renderer cannot be used \
             in process and the scene is handed to the `moonray` binary \
             instead"
        );
        return false;
    };

    // One renderer per process is MoonRay's own constraint -- its
    // driver state is global (`002` `research.md` F4) -- so this can
    // legitimately answer `None` while another render is running.
    let Some(render) = crate::rdl2::Render::new(
        Some(&dso),
        None,
        crate::rdl2::Mode::Progressive,
    ) else {
        eprintln!(
            "nsi-moonray: a MoonRay render is already running in this \
             process; this scene goes to the `moonray` binary instead"
        );
        return false;
    };

    let flushed = flush(scene);
    for limitation in &flushed.limitations {
        eprintln!("nsi-moonray: {limitation}");
    }

    // `.rdla` is a dump now, not the transport -- but a dump you can
    // still ask for, and `$NSI_MOONRAY_SCENE` is how. It has to work on
    // *this* path too: the scene someone wants to look at is the one
    // that actually rendered, and only writing it on the fallback would
    // hand them the wrong answer or nothing at all.
    dump_scene(&flushed);

    // The renderer owns the scene, so this is the context the frame is
    // rendered from rather than a copy pushed across.
    let Some(live) = render.scene() else {
        eprintln!("nsi-moonray: the renderer has no scene context");
        return false;
    };

    for line in crate::apply::apply(&flushed.document, &live) {
        eprintln!("nsi-moonray: {line}");
    }

    if let Err(error) = render.initialize() {
        // Not a reason to give up on the scene: fall back to the
        // spawned binary, which reports rather than crashes. A scene
        // with no camera is the common way here (see the shim).
        eprintln!(
            "nsi-moonray: render prep failed in process ({}); handing \
             the scene to the `moonray` binary instead",
            render.error().unwrap_or_else(|| error.to_string())
        );
        return false;
    }
    if let Err(error) = render.start() {
        eprintln!("nsi-moonray: the frame did not start: {error}");
        return false;
    }

    let drivers: Vec<(String, Callbacks)> = scene
        .nodes()
        .filter(|(_, node)| node.node_type == "outputdriver")
        .filter_map(|(handle, _)| {
            Some((handle.clone(), Callbacks::of(scene, handle)?))
        })
        .collect();

    match drivers.first() {
        // An application with a viewport: stream to it.
        Some((handle, callbacks)) => {
            if drivers.len() > 1 {
                eprintln!(
                    "nsi-moonray: {} output drivers carry callbacks; only \
                     {handle:?} is streamed to so far",
                    drivers.len()
                );
            }
            if let Err(error) =
                crate::stream::stream(&render, callbacks, handle, None)
            {
                eprintln!("nsi-moonray: {handle:?} streaming: {error}");
            }
        }
        // No callbacks: a batch render whose outputs are files.
        // Converge, then stop -- and say the file is not written,
        // rather than leave someone hunting for it.
        None => {
            while !render.frame_complete() {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let _ = render.stop();
            eprintln!(
                "nsi-moonray: rendered in process, but no `outputdriver` \
                 carries callbacks and writing the image to a file from \
                 the linked renderer is not wired up yet, so nothing was \
                 saved"
            );
        }
    }

    true
}

/// Write the `.rdla` dump, if one was asked for.
///
/// Only when `$NSI_MOONRAY_SCENE` names a path: on the in-process path
/// nothing needs a file, so writing one unasked would leave litter next
/// to whatever ran the render.
#[cfg(all(feature = "rdl2", moonray))]
fn dump_scene(flushed: &crate::flush::Flushed) {
    let Some(path) = std::env::var_os("NSI_MOONRAY_SCENE") else {
        return;
    };
    let path = PathBuf::from(path);

    if let Err(error) = std::fs::write(&path, flushed.to_rdla()) {
        eprintln!(
            "nsi-moonray: cannot write the scene dump {}: {error}",
            path.display()
        );
    }
}

/// Where MoonRay's scene classes live.
///
/// `$NSI_MOONRAY_DSO` names the directory outright; `$MOONRAY_ROOT`
/// names an install and `rdl2dso` is found under it.
#[cfg(all(feature = "rdl2", moonray))]
fn moonray_dso_path() -> Option<String> {
    if let Some(path) = std::env::var_os("NSI_MOONRAY_DSO") {
        return Some(path.to_string_lossy().into_owned());
    }

    let root = std::env::var_os("MOONRAY_ROOT")?;
    let path = PathBuf::from(root).join("rdl2dso");
    path.is_dir().then(|| path.to_string_lossy().into_owned())
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
