//! A live `scene_rdl2` scene, over the shim in `shim/`.
//!
//! This is what makes MoonRay *linked* rather than spawned, and that
//! distinction is the point rather than a detail: a separate process
//! has no `SceneContext` to edit and no `RenderContext` to snapshot, so
//! spawning forecloses incremental updates, progressive delivery and
//! concurrent rendering all at once. See
//! `specs/002-interactive-updates`.
//!
//! # What this is not
//!
//! It is not a general `scene_rdl2` binding. It carries exactly what a
//! [`Document`](crate::document::Document) needs replaying, which is
//! why the surface is a list of typed setters rather than anything
//! generic.
//!
//! # Errors are reports, not refusals
//!
//! ɴsɪ always returns an image. Every call here answers with an
//! [`Error`] the caller collects into limitations, and a scene with one
//! unmappable attribute still renders. The two ways a `set` fails stay
//! *distinguishable* on purpose: [`Error::NoSuchAttribute`] is a
//! mapping this backend has not written, [`Error::TypeMismatch`] is one
//! it wrote wrongly, and a report that conflates them helps nobody.

use std::ffi::{CString, c_char, c_int, c_void};

mod ffi;

/// What a shim call answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The object's class declares no attribute of that name. A mapping
    /// not yet written.
    NoSuchAttribute,
    /// The attribute exists but is not of the type written. A mapping
    /// written wrongly.
    TypeMismatch,
    /// A null handle or an unusable value.
    BadArgument,
    /// Anything else rdl2 raised.
    Failed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoSuchAttribute => "no such attribute",
            Self::TypeMismatch => "attribute is of another type",
            Self::BadArgument => "bad argument",
            Self::Failed => "rdl2 refused the value",
        })
    }
}

impl std::error::Error for Error {}

fn result(code: c_int) -> Result<(), Error> {
    match code {
        ffi::NMR_OK => Ok(()),
        ffi::NMR_NO_SUCH_ATTRIBUTE => Err(Error::NoSuchAttribute),
        ffi::NMR_TYPE_MISMATCH => Err(Error::TypeMismatch),
        ffi::NMR_BAD_ARGUMENT => Err(Error::BadArgument),
        _ => Err(Error::Failed),
    }
}

/// Which motion sample a value belongs to.
///
/// rdl2 has exactly two, `TIMESTEP_BEGIN` and `TIMESTEP_END`. ɴsɪ has
/// no such limit, so a scene with three samples on one attribute cannot
/// be carried across and the reduction is reported rather than made
/// quietly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Timestep {
    /// Not motion data: the value holds for the whole shutter.
    #[default]
    WholeShutter,
    Begin,
    End,
}

impl Timestep {
    fn code(self) -> c_int {
        match self {
            Self::WholeShutter => ffi::NMR_WHOLE_SHUTTER,
            Self::Begin => ffi::NMR_TIMESTEP_BEGIN,
            Self::End => ffi::NMR_TIMESTEP_END,
        }
    }
}

/// A live scene.
///
/// Owns every object it hands out; those stay valid until it is
/// dropped, which is why [`Object`] borrows it.
pub struct Context {
    raw: *mut ffi::NmrContext,
}

// SAFETY: a `SceneContext` is not internally synchronised, so this is
// `Send` (it can be moved to another thread) but not `Sync` (it cannot
// be shared without one). rdl2's own update protocol --
// `beginUpdate`/`endUpdate` -- is what serialises edits.
unsafe impl Send for Context {}

impl Context {
    /// A new scene context.
    ///
    /// `dso_path` is where rdl2 looks for scene classes. MoonRay's own
    /// DSOs -- `RdlMeshGeometry`, `UsdPreviewSurface`, `EnvLight` --
    /// live in its install's `rdl2dso`, so without this every
    /// [`Context::object`] for a MoonRay class answers `None`.
    pub fn new(dso_path: Option<&str>) -> Option<Self> {
        let path = dso_path.and_then(|p| CString::new(p).ok());
        let pointer = path.as_ref().map_or(std::ptr::null(), |p| p.as_ptr());

        // SAFETY: `pointer` is a valid NUL-terminated string or null,
        // which the shim accepts.
        let raw = unsafe { ffi::nmr_context_new(pointer) };
        (!raw.is_null()).then_some(Self { raw })
    }

    /// Create the object, or return the one already of that name.
    ///
    /// Create-or-get, which is rdl2's own behaviour: it is what makes
    /// flushing the same scene twice idempotent rather than an error,
    /// and what an incremental apply relies on to reach an object it
    /// created a frame ago.
    ///
    /// `None` means the class could not be loaded, which is the
    /// ordinary way a missing DSO shows up. [`Context::error`] says
    /// which.
    pub fn object<'a>(&'a self, class: &str, name: &str) -> Option<Object<'a>> {
        let class = CString::new(class).ok()?;
        let name = CString::new(name).ok()?;

        // SAFETY: both strings are valid and NUL-terminated, and the
        // context is live.
        let raw =
            unsafe { ffi::nmr_object(self.raw, class.as_ptr(), name.as_ptr()) };

        (!raw.is_null()).then_some(Object {
            raw,
            _context: std::marker::PhantomData,
        })
    }

    /// What rdl2 last complained about, if anything.
    pub fn error(&self) -> Option<String> {
        // SAFETY: the shim owns the string and keeps it until the next
        // failing call; it is copied out before anything else runs.
        unsafe {
            let message = ffi::nmr_context_error(self.raw);
            (!message.is_null()).then(|| {
                std::ffi::CStr::from_ptr(message)
                    .to_string_lossy()
                    .into_owned()
            })
        }
    }

    /// Write the live scene out as `.rdla`.
    ///
    /// This is what keeps the emitter honest now that `.rdla` is no
    /// longer the transport: dump the applied scene and diff it against
    /// what [`Document::to_rdla`](crate::document::Document::to_rdla)
    /// writes for the same input.
    pub fn write_ascii(&self, path: &std::path::Path) -> Result<(), Error> {
        let path = CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|_| Error::BadArgument)?;

        // SAFETY: a valid context and a NUL-terminated path.
        result(unsafe { ffi::nmr_context_write_ascii(self.raw, path.as_ptr()) })
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: created by `nmr_context_new` and freed exactly once.
        unsafe { ffi::nmr_context_free(self.raw) }
    }
}

/// One `SceneObject`, borrowed from the [`Context`] that owns it.
#[derive(Debug, Clone, Copy)]
pub struct Object<'a> {
    raw: *mut ffi::NmrObject,
    _context: std::marker::PhantomData<&'a Context>,
}

/// rdl2 requires every `set` to happen inside an update.
///
/// Held as a guard rather than left to the caller because forgetting
/// `endUpdate` does not error -- it leaves the object marked dirty and
/// the renderer doing work it need not, which is invisible in the
/// image and shows up only as time.
pub struct Update<'a> {
    object: Object<'a>,
}

impl<'a> Object<'a> {
    /// Open an update. The guard closes it.
    pub fn update(self) -> Result<Update<'a>, Error> {
        // SAFETY: a live object from a live context.
        result(unsafe { ffi::nmr_begin_update(self.raw) })?;
        Ok(Update { object: self })
    }

    fn name(name: &str) -> Result<CString, Error> {
        CString::new(name).map_err(|_| Error::BadArgument)
    }
}

impl Drop for Update<'_> {
    fn drop(&mut self) {
        // SAFETY: opened by `update`, closed exactly once.
        unsafe { ffi::nmr_end_update(self.object.raw) };
    }
}

/// A setter that takes one scalar.
macro_rules! scalar {
    ($name:ident, $shim:ident, $type:ty) => {
        pub fn $name(
            &self,
            attribute: &str,
            value: $type,
            timestep: Timestep,
        ) -> Result<(), Error> {
            let attribute = Object::name(attribute)?;
            // SAFETY: a live object, a NUL-terminated name, and a
            // by-value scalar.
            result(unsafe {
                ffi::$shim(
                    self.object.raw,
                    attribute.as_ptr(),
                    value,
                    timestep.code(),
                )
            })
        }
    };
}

/// A setter that takes a fixed-size tuple by pointer.
macro_rules! tuple {
    ($name:ident, $shim:ident, $type:ty, $count:literal) => {
        pub fn $name(
            &self,
            attribute: &str,
            value: &[$type; $count],
            timestep: Timestep,
        ) -> Result<(), Error> {
            let attribute = Object::name(attribute)?;
            // SAFETY: the array is exactly the length the shim reads.
            result(unsafe {
                ffi::$shim(
                    self.object.raw,
                    attribute.as_ptr(),
                    value.as_ptr(),
                    timestep.code(),
                )
            })
        }
    };
}

/// A setter that takes a flat buffer of components.
macro_rules! vector {
    ($name:ident, $shim:ident, $type:ty, $components:literal) => {
        /// `values` is flat: `count * components` long.
        pub fn $name(
            &self,
            attribute: &str,
            values: &[$type],
        ) -> Result<(), Error> {
            let attribute = Object::name(attribute)?;
            if values.len() % $components != 0 {
                return Err(Error::BadArgument);
            }
            // SAFETY: the count passed is derived from the slice's own
            // length, so the shim reads exactly what is there.
            result(unsafe {
                ffi::$shim(
                    self.object.raw,
                    attribute.as_ptr(),
                    values.as_ptr(),
                    values.len() / $components,
                )
            })
        }
    };
}

impl Update<'_> {
    scalar!(set_int, nmr_set_int, i32);
    scalar!(set_long, nmr_set_long, i64);
    scalar!(set_float, nmr_set_float, f32);
    scalar!(set_double, nmr_set_double, f64);

    tuple!(set_rgb, nmr_set_rgb, f32, 3);
    tuple!(set_rgba, nmr_set_rgba, f32, 4);
    tuple!(set_vec2f, nmr_set_vec2f, f32, 2);
    tuple!(set_vec2d, nmr_set_vec2d, f64, 2);
    tuple!(set_vec3f, nmr_set_vec3f, f32, 3);
    tuple!(set_vec3d, nmr_set_vec3d, f64, 3);
    tuple!(set_vec4f, nmr_set_vec4f, f32, 4);
    tuple!(set_vec4d, nmr_set_vec4d, f64, 4);
    tuple!(set_mat4f, nmr_set_mat4f, f32, 16);
    tuple!(set_mat4d, nmr_set_mat4d, f64, 16);

    vector!(set_int_vector, nmr_set_int_vector, i32, 1);
    vector!(set_long_vector, nmr_set_long_vector, i64, 1);
    vector!(set_float_vector, nmr_set_float_vector, f32, 1);
    vector!(set_double_vector, nmr_set_double_vector, f64, 1);
    vector!(set_rgb_vector, nmr_set_rgb_vector, f32, 3);
    vector!(set_vec2f_vector, nmr_set_vec2f_vector, f32, 2);
    vector!(set_vec3f_vector, nmr_set_vec3f_vector, f32, 3);
    vector!(set_vec3d_vector, nmr_set_vec3d_vector, f64, 3);
    vector!(set_vec4f_vector, nmr_set_vec4f_vector, f32, 4);
    vector!(set_mat4f_vector, nmr_set_mat4f_vector, f32, 16);
    vector!(set_mat4d_vector, nmr_set_mat4d_vector, f64, 16);

    pub fn set_bool(
        &self,
        attribute: &str,
        value: bool,
        timestep: Timestep,
    ) -> Result<(), Error> {
        let attribute = Object::name(attribute)?;
        // SAFETY: as the scalar setters; C takes the bool as an int.
        result(unsafe {
            ffi::nmr_set_bool(
                self.object.raw,
                attribute.as_ptr(),
                c_int::from(value),
                timestep.code(),
            )
        })
    }

    pub fn set_string(
        &self,
        attribute: &str,
        value: &str,
        timestep: Timestep,
    ) -> Result<(), Error> {
        let attribute = Object::name(attribute)?;
        let value = CString::new(value).map_err(|_| Error::BadArgument)?;
        // SAFETY: both strings are NUL-terminated and outlive the call,
        // which copies into a `std::string`.
        result(unsafe {
            ffi::nmr_set_string(
                self.object.raw,
                attribute.as_ptr(),
                value.as_ptr(),
                timestep.code(),
            )
        })
    }

    /// A `SceneObject*` attribute. `None` is rdl2's `undef()`.
    ///
    /// Writing `None` is a decision rather than an absence: MoonRay does
    /// not render a `Layer` row whose material column is undefined.
    pub fn set_object(
        &self,
        attribute: &str,
        value: Option<Object<'_>>,
        timestep: Timestep,
    ) -> Result<(), Error> {
        let attribute = Object::name(attribute)?;
        // SAFETY: the target belongs to the same live context.
        result(unsafe {
            ffi::nmr_set_object(
                self.object.raw,
                attribute.as_ptr(),
                value.map_or(std::ptr::null_mut(), |o| o.raw),
                timestep.code(),
            )
        })
    }

    pub fn set_string_vector(
        &self,
        attribute: &str,
        values: &[&str],
    ) -> Result<(), Error> {
        let attribute = Object::name(attribute)?;
        let owned: Vec<CString> = values
            .iter()
            .map(|value| CString::new(*value))
            .collect::<Result<_, _>>()
            .map_err(|_| Error::BadArgument)?;
        let pointers: Vec<*const c_char> =
            owned.iter().map(|value| value.as_ptr()).collect();

        // SAFETY: `owned` outlives the call, so every pointer in
        // `pointers` is live while the shim copies from it.
        result(unsafe {
            ffi::nmr_set_string_vector(
                self.object.raw,
                attribute.as_ptr(),
                pointers.as_ptr(),
                pointers.len(),
            )
        })
    }

    pub fn set_object_vector(
        &self,
        attribute: &str,
        values: &[Object<'_>],
    ) -> Result<(), Error> {
        let attribute = Object::name(attribute)?;
        let pointers: Vec<*mut ffi::NmrObject> =
            values.iter().map(|object| object.raw).collect();

        // SAFETY: every object belongs to the same live context.
        result(unsafe {
            ffi::nmr_set_object_vector(
                self.object.raw,
                attribute.as_ptr(),
                pointers.as_ptr(),
                pointers.len(),
            )
        })
    }

    /// Bind an attribute to another object -- where an ɴsɪ named shader
    /// port lands. The attribute keeps its own value alongside the
    /// binding, which the format oracle settled.
    pub fn set_binding(
        &self,
        attribute: &str,
        target: Option<Object<'_>>,
    ) -> Result<(), Error> {
        let attribute = Object::name(attribute)?;
        // SAFETY: as `set_object`.
        result(unsafe {
            ffi::nmr_set_binding(
                self.object.raw,
                attribute.as_ptr(),
                target.map_or(std::ptr::null_mut(), |o| o.raw),
            )
        })
    }

    /// Add a member to a `GeometrySet` or `LightSet`.
    pub fn add(&self, member: Object<'_>) -> Result<(), Error> {
        // SAFETY: both belong to the same live context.
        result(unsafe { ffi::nmr_set_add(self.object.raw, member.raw) })
    }

    /// One `Layer` row.
    pub fn assign(
        &self,
        geometry: Object<'_>,
        part: &str,
        material: Option<Object<'_>>,
        light_set: Option<Object<'_>>,
    ) -> Result<(), Error> {
        let part = CString::new(part).map_err(|_| Error::BadArgument)?;
        // SAFETY: every object belongs to the same live context, and
        // `part` is NUL-terminated.
        result(unsafe {
            ffi::nmr_layer_assign(
                self.object.raw,
                geometry.raw,
                part.as_ptr(),
                material.map_or(std::ptr::null_mut(), |o| o.raw),
                light_set.map_or(std::ptr::null_mut(), |o| o.raw),
            )
        })
    }
}

// Silences the unused-import warning on a build where no setter takes
// one; `c_void` is what the opaque handles are.
const _: Option<*mut c_void> = None;
