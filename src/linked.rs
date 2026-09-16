//! This backend as an ɴsɪ implementation a Rust host links, rather than
//! loads.
//!
//! # Why this exists at all
//!
//! The C entry points in [`crate::capi`] are the drop-in route: build
//! the `cdylib`, point an application's loader at it, and it renders. It
//! works for any host in any language, and it is how a host keeps the
//! ability to ship without a renderer.
//!
//! It has one hole, and it is the interesting one. ɴsɪ delivers pixels
//! to an `outputdriver`'s `callback.open` / `callback.write` /
//! `callback.finish`, and in `nsi-ffi-wrap` those are `Box<dyn Fn…>` --
//! Rust trait objects. **A trait object's vtable belongs to the
//! compilation that produced it**, and Rust has no stable ABI, so a
//! library loaded at run time is a different compilation whose idea of
//! that vtable is unrelated. Calling such a closure is undefined
//! behaviour, not a feature that happens to be missing. Passing the
//! pointer through the C interface is *how* it crosses, not *why* it
//! breaks: the same hazard exists whatever the pointer travels in.
//!
//! A host that links this crate has no such boundary. One compilation,
//! one vtable, native closures. The pixels go
//!
//! ```text
//! application's closure  ->  this crate  ->  MoonRay
//!         Rust                   Rust           C++
//! ```
//!
//! with nothing ABI-stable needed in the middle, because there is no
//! middle.
//!
//! # Using it
//!
//! ```ignore
//! let context = nsi::Context::new(Some(&[
//!     nsi::string!("renderer", "moonray"),
//! ]));
//! ```
//!
//! Usually no call to make first: a `#[ctor]` function below calls
//! `nsi_ffi_wrap::backend::register` before `main` runs, the same way
//! `nsi::backend::register("moonray",
//! std::sync::Arc::new(nsi_moonray::MoonRay))` would by hand. "Usually"
//! is doing real work in that sentence -- read on before leaving the
//! call out.
//!
//! # This is best-effort, not a guarantee
//!
//! **A `#[ctor]` inside a dependency's `rlib` is not reliably linked
//! into a consumer that never references anything else from it.** This
//! is not specific to this crate or to `ctor`: it is how static
//! linking works. `rustc`/`ld` pull object files out of an archive
//! only for symbols something actually references: a `#[used]` static
//! keeps a *linked* object file's section from being garbage-collected,
//! but it cannot make the linker select an object file that nothing
//! names in the first place. Nothing else in a host that only calls
//! `nsi::Context::new(&[renderer("moonray")])` references anything in
//! this module, so the linker has no reason to pull this file's object
//! code from the archive, and the constructor that would have
//! registered `"moonray"` is never linked in to run. Cargo has no
//! stable, portable way for a library to force `--whole-archive`
//! linking onto an arbitrary downstream consumer -- see
//! <https://github.com/rust-lang/cargo/issues/7586>, open since 2019 --
//! so this cannot be closed from this crate alone.
//!
//! It *does* run reliably when this crate is loaded as the `cdylib`
//! (a shared library links every reachable symbol, archive-selection
//! does not apply), and it runs incidentally whenever a host's own
//! code references anything else in this module -- holding an
//! `Arc<MoonRay>` for some other reason, say. Measured here: present
//! and firing in `libnsi_moonray.so`, absent from a `cargo test`
//! binary that named nothing else in this file.
//! `tests/shaderballs.rs`'s `a_row_of_looks_through_both_renderers`
//! hit exactly this, which is how it was found rather than assumed.
//!
//! **So: call `register` explicitly unless you already know your
//! binary references something else here.** It is idempotent --
//! registering a name twice keeps the later write -- so calling it
//! even when the `#[ctor]` also fires costs nothing and is the safe
//! default. This crate's own tests do.
//!
//! **The host and this crate must share one `nsi-ffi-wrap`.** That is
//! the whole point, and Cargo gives it for free within one build graph
//! -- but only if nothing patches the two apart. A workspace overriding
//! `nsi` to a path must override `nsi-ffi-wrap` to the matching path
//! too, or the closures are foreign again and the hazard is back with
//! no boundary to make it obvious.
//!
//! # What is *not* different
//!
//! Everything else. The scene, the flush, the renderer and every
//! limitation report are the same code the C entry points run: this
//! forwards to them rather than reimplementing anything, so the two
//! routes cannot drift into rendering differently.

use crate::capi;
use nsi_ffi_wrap::{
    FfiApi,
    nsi_sys::{NSIContext, NSIHandle, NSIParam},
};
use nsi_trait::FfiParam;
use std::{ffi::c_char, os::raw::c_int};

/// This backend, as something a host registers.
///
/// Stateless: every context lives in [`crate::capi`]'s own table, keyed
/// by the handle `NSIBegin` returns, exactly as it does for a loaded
/// library. So one of these answers for every context at once, and
/// cloning it costs nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct MoonRay;

/// Registers [`MoonRay`] with `nsi-ffi-wrap` before `main` runs --
/// **when this object file is linked in at all.** See the module doc
/// comment's "This is best-effort, not a guarantee" section: nothing
/// makes that certain for a host that references nothing else here,
/// and measurement rather than the mechanism's own description is
/// what found that out.
///
/// Unconditional since `nsi-ffi-wrap` 0.10.3, which is where
/// `nsi_ffi_wrap::backend` first shipped. A host that does not want
/// `MoonRay` to answer `"moonray"` can call
/// `nsi_ffi_wrap::backend::register` itself afterwards with something
/// else; registering a name twice keeps the later one.
///
/// `ctor`'s mechanism -- a function placed in a platform's pre-`main`
/// init section (`.init_array` on Linux, and the equivalents
/// elsewhere) -- is what lets this run with no call site, on the
/// binaries it does reach. `backend::register`'s own storage is a
/// `LazyLock<Mutex<...>>`, which initialises itself on first touch and
/// needs nothing already running to be safe to call this early --
/// that part was never in question; being linked in at all was.
///
/// # Safety
///
/// `ctor` 1.0 requires this acknowledgement because a pre-`main`
/// function runs before the platform guarantees anything is set up,
/// and it cannot verify that for an arbitrary function body. This one
/// only allocates (a `String` key, a `HashMap` entry) and takes a
/// `Mutex` -- both routed through the same allocator and libc `main`
/// itself will use once it starts, and neither needs a thread pool,
/// signal handlers, or any other runtime service `.init_array` runs
/// before. No global state is read here, only written, so there is
/// nothing for run order against another `ctor` to corrupt.
#[ctor::ctor(unsafe)]
fn register_with_nsi() {
    nsi_ffi_wrap::backend::register("moonray", std::sync::Arc::new(MoonRay));
}

/// `nsi_sys::NSIParam` and `nsi_trait::FfiParam` are both `#[repr(C)]`
/// mirrors of the specification's `NSIParam_t`, field for field. The
/// cast is between two spellings of one C struct.
#[inline]
fn params(params: *const NSIParam) -> *const FfiParam {
    params.cast()
}

// **`FfiApi`'s methods are safe-by-signature and take raw pointers**,
// which is `not_unsafe_ptr_arg_deref`'s exact complaint -- and the
// signature is `nsi-ffi-wrap`'s, not this crate's, so it cannot be
// answered by marking them `unsafe`. The contract is the ɴsɪ
// interface's own: a caller passes `nparams` valid parameters. Every
// method forwards to the C entry point written against that same
// contract, and each one carries the `SAFETY` note for what it
// forwards.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl FfiApi for MoonRay {
    fn NSIBegin(&self, nparams: c_int, p: *const NSIParam) -> NSIContext {
        // SAFETY: the caller of the ɴsɪ interface guarantees `nparams`
        // valid parameters, which is the same contract the C entry
        // point is written against. Every method below is the same.
        unsafe { capi::NSIBegin(nparams, params(p)) }
    }

    fn NSIEnd(&self, ctx: NSIContext) {
        // Safe, unlike its neighbours: a context handle is an integer
        // and there are no parameters to trust.
        capi::NSIEnd(ctx)
    }

    fn NSICreate(
        &self,
        ctx: NSIContext,
        handle: NSIHandle,
        type_: *const c_char,
        nparams: c_int,
        p: *const NSIParam,
    ) {
        unsafe { capi::NSICreate(ctx, handle, type_, nparams, params(p)) }
    }

    fn NSIDelete(
        &self,
        ctx: NSIContext,
        handle: NSIHandle,
        nparams: c_int,
        p: *const NSIParam,
    ) {
        unsafe { capi::NSIDelete(ctx, handle, nparams, params(p)) }
    }

    fn NSISetAttribute(
        &self,
        ctx: NSIContext,
        object: NSIHandle,
        nparams: c_int,
        p: *const NSIParam,
    ) {
        unsafe { capi::NSISetAttribute(ctx, object, nparams, params(p)) }
    }

    fn NSISetAttributeAtTime(
        &self,
        ctx: NSIContext,
        object: NSIHandle,
        time: f64,
        nparams: c_int,
        p: *const NSIParam,
    ) {
        unsafe {
            capi::NSISetAttributeAtTime(ctx, object, time, nparams, params(p))
        }
    }

    fn NSIDeleteAttribute(
        &self,
        ctx: NSIContext,
        object: NSIHandle,
        name: *const c_char,
    ) {
        unsafe { capi::NSIDeleteAttribute(ctx, object, name) }
    }

    fn NSIConnect(
        &self,
        ctx: NSIContext,
        from: NSIHandle,
        from_attribute: *const c_char,
        to: NSIHandle,
        to_attribute: *const c_char,
        nparams: c_int,
        p: *const NSIParam,
    ) {
        unsafe {
            capi::NSIConnect(
                ctx,
                from,
                from_attribute,
                to,
                to_attribute,
                nparams,
                params(p),
            )
        }
    }

    fn NSIDisconnect(
        &self,
        ctx: NSIContext,
        from: NSIHandle,
        from_attribute: *const c_char,
        to: NSIHandle,
        to_attribute: *const c_char,
    ) {
        unsafe {
            capi::NSIDisconnect(ctx, from, from_attribute, to, to_attribute)
        }
    }

    fn NSIEvaluate(&self, ctx: NSIContext, nparams: c_int, p: *const NSIParam) {
        unsafe { capi::NSIEvaluate(ctx, nparams, params(p)) }
    }

    fn NSIRenderControl(
        &self,
        ctx: NSIContext,
        nparams: c_int,
        p: *const NSIParam,
    ) {
        unsafe { capi::NSIRenderControl(ctx, nparams, params(p)) }
    }

    /// **Registering a display driver is for the loaded route.**
    ///
    /// A host that linked this crate hands its pixels over as closures
    /// on the `outputdriver` node, which is the whole reason to link.
    /// The call is still forwarded rather than refused: a host may
    /// register one anyway, and a driver that is registered and never
    /// used costs nothing.
    //
    // Not feature-gated here: the trait declares this method whenever
    // `nsi-ffi-wrap` has its `output` feature, which this crate turns on
    // unconditionally because `display.rs` needs it.
    fn DspyRegisterDriver(
        &self,
        driver_name: *const c_char,
        p_open: ndspy_sys::PtDspyOpenFuncPtr,
        p_write: ndspy_sys::PtDspyWriteFuncPtr,
        p_close: ndspy_sys::PtDspyCloseFuncPtr,
        p_query: ndspy_sys::PtDspyQueryFuncPtr,
    ) -> ndspy_sys::PtDspyError {
        // SAFETY: the pointers are the host's own function pointers, or
        // null; `capi::DspyRegisterDriver` stores them and calls them
        // only through the ɴsɪ output path.
        let code = unsafe {
            capi::DspyRegisterDriver(
                driver_name,
                p_open.map_or(std::ptr::null(), |f| f as *const _),
                p_write.map_or(std::ptr::null(), |f| f as *const _),
                p_close.map_or(std::ptr::null(), |f| f as *const _),
                p_query.map_or(std::ptr::null(), |f| f as *const _),
            )
        };

        // The C entry point answers in the interface's integers;
        // `ndspy-sys` spells the same values as an enum, and an
        // unrecognised one is `Undefined` rather than a transmute into a
        // variant that does not exist.
        match code {
            0 => ndspy_sys::PtDspyError::None,
            1 => ndspy_sys::PtDspyError::NoMemory,
            2 => ndspy_sys::PtDspyError::Unsupported,
            3 => ndspy_sys::PtDspyError::BadParams,
            4 => ndspy_sys::PtDspyError::NoResource,
            6 => ndspy_sys::PtDspyError::Stop,
            _ => ndspy_sys::PtDspyError::Undefined,
        }
    }
}
