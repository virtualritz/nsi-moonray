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
//! nsi::backend::register("moonray", std::sync::Arc::new(nsi_moonray::MoonRay));
//!
//! let context = nsi::Context::new(Some(&[
//!     nsi::string!("renderer", "moonray"),
//! ]));
//! ```
//!
//! Register before the first context that asks for the name; one
//! already built keeps the renderer it was built with.
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
