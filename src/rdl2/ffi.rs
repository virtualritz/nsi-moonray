//! The raw declarations, mirroring `shim/include/nsi_moonray_shim.h`.
//!
//! Kept apart from the safe layer so the two can be read against each
//! other: every function here has exactly one caller in `mod.rs`, and
//! the header is the third copy. A mismatch between them is undefined
//! behaviour with no diagnostic, so they are ordered the same way.

#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int};

pub const NMR_OK: c_int = 0;
pub const NMR_NO_SUCH_ATTRIBUTE: c_int = 1;
pub const NMR_TYPE_MISMATCH: c_int = 2;
pub const NMR_BAD_ARGUMENT: c_int = 3;

pub const NMR_WHOLE_SHUTTER: c_int = -1;
pub const NMR_TIMESTEP_BEGIN: c_int = 0;
pub const NMR_TIMESTEP_END: c_int = 1;

/// Opaque; the shim owns it.
#[repr(C)]
pub struct NmrContext {
    _private: [u8; 0],
}

/// Opaque, and owned by the context rather than by this.
#[repr(C)]
pub struct NmrObject {
    _private: [u8; 0],
}

/// Opaque; owns the `SceneContext` it renders.
#[cfg(moonray)]
#[repr(C)]
pub struct NmrRender {
    _private: [u8; 0],
}

unsafe extern "C" {
    pub fn nmr_context_new(dso_path: *const c_char) -> *mut NmrContext;
    pub fn nmr_context_free(context: *mut NmrContext);
    pub fn nmr_context_error(context: *const NmrContext) -> *const c_char;
    pub fn nmr_context_write_ascii(
        context: *mut NmrContext,
        path: *const c_char,
    ) -> c_int;

    pub fn nmr_object(
        context: *mut NmrContext,
        class_name: *const c_char,
        object_name: *const c_char,
    ) -> *mut NmrObject;

    pub fn nmr_begin_update(object: *mut NmrObject) -> c_int;
    pub fn nmr_end_update(object: *mut NmrObject) -> c_int;

    pub fn nmr_set_bool(
        o: *mut NmrObject,
        name: *const c_char,
        value: c_int,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_int(
        o: *mut NmrObject,
        name: *const c_char,
        value: i32,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_long(
        o: *mut NmrObject,
        name: *const c_char,
        value: i64,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_float(
        o: *mut NmrObject,
        name: *const c_char,
        value: f32,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_double(
        o: *mut NmrObject,
        name: *const c_char,
        value: f64,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_string(
        o: *mut NmrObject,
        name: *const c_char,
        value: *const c_char,
        timestep: c_int,
    ) -> c_int;

    pub fn nmr_set_rgb(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_rgba(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_vec2f(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_vec2d(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f64,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_vec3f(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_vec3d(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f64,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_vec4f(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_vec4d(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f64,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_mat4f(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        timestep: c_int,
    ) -> c_int;
    pub fn nmr_set_mat4d(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f64,
        timestep: c_int,
    ) -> c_int;

    pub fn nmr_set_object(
        o: *mut NmrObject,
        name: *const c_char,
        value: *mut NmrObject,
        timestep: c_int,
    ) -> c_int;

    pub fn nmr_set_int_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const i32,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_long_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const i64,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_float_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_double_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f64,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_string_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const *const c_char,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_rgb_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_vec2f_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_vec3f_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_vec3d_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f64,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_vec4f_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_mat4f_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f32,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_mat4d_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const f64,
        count: usize,
    ) -> c_int;
    pub fn nmr_set_object_vector(
        o: *mut NmrObject,
        name: *const c_char,
        v: *const *mut NmrObject,
        count: usize,
    ) -> c_int;

    pub fn nmr_set_binding(
        o: *mut NmrObject,
        name: *const c_char,
        target: *mut NmrObject,
    ) -> c_int;

    pub fn nmr_set_add(set: *mut NmrObject, member: *mut NmrObject) -> c_int;

    pub fn nmr_layer_assign(
        layer: *mut NmrObject,
        geometry: *mut NmrObject,
        part: *const c_char,
        material: *mut NmrObject,
        light_set: *mut NmrObject,
    ) -> c_int;
}

#[cfg(moonray)]
unsafe extern "C" {
    pub fn nmr_render_new(
        dso_path: *const c_char,
        threads: u32,
        mode: c_int,
    ) -> *mut NmrRender;
    pub fn nmr_render_free(render: *mut NmrRender);
    pub fn nmr_render_error(render: *const NmrRender) -> *const c_char;
    pub fn nmr_render_scene(render: *mut NmrRender) -> *mut NmrContext;
    pub fn nmr_render_initialize(render: *mut NmrRender) -> c_int;
    pub fn nmr_render_start(render: *mut NmrRender) -> c_int;
    pub fn nmr_render_stop(render: *mut NmrRender) -> c_int;
    pub fn nmr_render_is_ready_for_display(render: *const NmrRender) -> c_int;
    pub fn nmr_render_are_coarse_passes_complete(
        render: *const NmrRender,
    ) -> c_int;
    pub fn nmr_render_is_frame_complete(render: *const NmrRender) -> c_int;
    pub fn nmr_render_resolution(
        render: *const NmrRender,
        width: *mut u32,
        height: *mut u32,
    ) -> c_int;
    pub fn nmr_render_snapshot(
        render: *mut NmrRender,
        pixels: *mut f32,
        capacity: usize,
    ) -> c_int;
}
