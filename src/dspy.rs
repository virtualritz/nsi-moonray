//! The display-driver API a real application actually uses.
//!
//! ɴsɪ deliberately leaves the driver interface unspecified -- the
//! specification says it is "implementation specific and not covered by
//! this documentation" -- and in practice everyone settled on
//! RenderMan's `Dspy`. Gaffer registers a driver called `ieDisplay` and
//! sets `outputdriver.drivername` to it; Houdini registers its viewport
//! driver and `idisplay`.
//!
//! **Until this existed no application received a pixel.**
//! `DspyRegisterDriver` was a stub that printed a line and returned
//! success, `DspyRegisterDriverTable` was not exported at all, and
//! `drivername` was ignored, so a host's viewport stayed empty while
//! the render succeeded. The other delivery path -- Rust closures on an
//! `outputdriver` -- works only where the application and this backend
//! share one compilation of `nsi-ffi-wrap`, which a C++ host cannot do.
//!
//! # The ABI is not negotiable
//!
//! Every signature here is transcribed from `ndspy.h`, and a mistake in
//! one is a crash inside somebody else's application with a stack that
//! points at us. They are declared rather than bound so that this
//! module compiles on a machine with no RenderMan and no 3Delight,
//! which is every machine that builds this crate.

use std::{
    collections::HashMap,
    ffi::{CString, c_char, c_int, c_uchar, c_uint, c_void},
    sync::{LazyLock, Mutex},
};

/// `PtDspyError`. Zero is success; the rest are the interface's own.
pub type Error = c_int;

/// `PkDspyErrorNone`.
pub const OK: Error = 0;

/// An opaque per-image handle the driver hands back at open and
/// expects at write and close.
pub type ImageHandle = *mut c_void;

/// `UserParameter`, from `uparam.h`.
///
/// The extra attributes on an `outputdriver` node reach a driver
/// through an array of these, which is how Gaffer passes the port its
/// viewport listens on.
#[repr(C)]
pub struct UserParameter {
    pub name: *const c_char,
    pub value_type: c_char,
    pub value_count: c_char,
    pub value: *const c_void,
    pub nbytes: c_int,
}

/// `PtDspyDevFormat`: one channel's name and type.
#[repr(C)]
pub struct DevFormat {
    pub name: *const c_char,
    pub type_: c_uint,
}

/// `PkDspyFloat32`, the only format this backend delivers.
///
/// MoonRay snapshots float, so converting to anything narrower would
/// be a loss this side chose rather than one the driver asked for.
pub const FLOAT32: c_uint = 0;

/// `PtFlagStuff`.
#[repr(C)]
pub struct FlagStuff {
    pub flags: c_int,
}

pub type OpenFn = unsafe extern "C" fn(
    image: *mut ImageHandle,
    drivername: *const c_char,
    filename: *const c_char,
    width: c_int,
    height: c_int,
    param_count: c_int,
    parameters: *const UserParameter,
    format_count: c_int,
    format: *mut DevFormat,
    flags: *mut FlagStuff,
) -> Error;

pub type WriteFn = unsafe extern "C" fn(
    image: ImageHandle,
    xmin: c_int,
    xmax_plus_one: c_int,
    ymin: c_int,
    ymax_plus_one: c_int,
    entrysize: c_int,
    data: *const c_uchar,
) -> Error;

pub type CloseFn = unsafe extern "C" fn(image: ImageHandle) -> Error;

pub type QueryFn = unsafe extern "C" fn(
    image: ImageHandle,
    query: c_int,
    size: c_int,
    data: *mut c_void,
) -> Error;

pub type ActiveRegionFn = unsafe extern "C" fn(
    image: ImageHandle,
    xmin: c_int,
    xmax_plus_one: c_int,
    ymin: c_int,
    ymax_plus_one: c_int,
) -> Error;

/// `PtDspyDriverFunctionTable`.
#[repr(C)]
pub struct FunctionTable {
    pub version: c_int,
    pub open: Option<OpenFn>,
    pub write: Option<WriteFn>,
    pub close: Option<CloseFn>,
    pub query: Option<QueryFn>,
    pub active_region: Option<ActiveRegionFn>,
}

/// One registered driver.
///
/// `Copy` and pointer-sized: the registry is read on the render thread
/// for every bucket, so holding a lock across a driver call would
/// serialise delivery behind whatever the application does with it.
#[derive(Clone, Copy)]
pub struct Driver {
    pub open: Option<OpenFn>,
    pub write: Option<WriteFn>,
    pub close: Option<CloseFn>,
    pub query: Option<QueryFn>,
}

// SAFETY: these are plain function pointers into the host, which the
// interface's own contract says are callable from the renderer's
// threads. Nothing here is dereferenced except by calling it.
unsafe impl Send for Driver {}
unsafe impl Sync for Driver {}

/// Every driver an application has registered, by name.
///
/// Process-global because the registration entry points are: `Dspy`
/// has no context argument, and a host registers once before it opens
/// any ɴsɪ context.
static DRIVERS: LazyLock<Mutex<HashMap<String, Driver>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Record a driver under a name.
pub fn register(name: &str, driver: Driver) {
    if let Ok(mut drivers) = DRIVERS.lock() {
        drivers.insert(name.to_owned(), driver);
    }
}

/// Forget one. The interface spells unregistration as a null table.
pub fn unregister(name: &str) {
    if let Ok(mut drivers) = DRIVERS.lock() {
        drivers.remove(name);
    }
}

/// The driver an `outputdriver` node's `drivername` asks for.
pub fn lookup(name: &str) -> Option<Driver> {
    DRIVERS.lock().ok()?.get(name).copied()
}

/// The names registered, for a report that has to say what *was*
/// available when the one asked for was not.
pub fn registered() -> Vec<String> {
    DRIVERS
        .lock()
        .map(|drivers| drivers.keys().cloned().collect())
        .unwrap_or_default()
}

/// One open image, from `open` to `close`.
///
/// Holds the handle the driver gave back and the channel names it was
/// opened with, because `write` is told an entry size and nothing else
/// -- the layout was agreed at open and has to be remembered here.
pub struct Image {
    driver: Driver,
    handle: ImageHandle,
    /// Kept alive: the driver was handed pointers into these at open
    /// and the interface does not say it copied them.
    _channels: Vec<CString>,
    _name: CString,
    _driver_name: CString,
}

impl Image {
    /// Open an image on a driver.
    ///
    /// `channels` are the layer's own names -- MoonRay calls them
    /// `Ci.R`, `Ci.G`, `Ci.B` rather than `R`, `G`, `B`, and passing
    /// what the renderer actually produced is what lets an application
    /// find them.
    pub fn open(
        driver: Driver,
        driver_name: &str,
        file_name: &str,
        width: i32,
        height: i32,
        channels: &[String],
    ) -> Option<Self> {
        let open = driver.open?;

        let driver_name_c = CString::new(driver_name).ok()?;
        let file_name_c = CString::new(file_name).ok()?;
        let channels_c: Vec<CString> = channels
            .iter()
            .map(|name| CString::new(name.as_str()))
            .collect::<Result<_, _>>()
            .ok()?;

        let mut formats: Vec<DevFormat> = channels_c
            .iter()
            .map(|name| DevFormat {
                name: name.as_ptr(),
                type_: FLOAT32,
            })
            .collect();

        let mut handle: ImageHandle = std::ptr::null_mut();
        let mut flags = FlagStuff { flags: 0 };

        // SAFETY: every pointer is to something owned here and alive
        // across the call, and the counts match the slices.
        let status = unsafe {
            open(
                &mut handle,
                driver_name_c.as_ptr(),
                file_name_c.as_ptr(),
                width,
                height,
                0,
                std::ptr::null(),
                formats.len() as c_int,
                formats.as_mut_ptr(),
                &mut flags,
            )
        };

        if status != OK {
            return None;
        }

        Some(Self {
            driver,
            handle,
            _channels: channels_c,
            _name: file_name_c,
            _driver_name: driver_name_c,
        })
    }

    /// Hand one rectangle of pixels over.
    ///
    /// **The interface's `xmax` and `ymax` are one past the end**, which
    /// is the opposite of how MoonRay describes a bucket, and getting
    /// it wrong loses the last row and column of every rectangle --
    /// visible only as a grid of seams.
    pub fn write(
        &self,
        xmin: i32,
        ymin: i32,
        width: i32,
        height: i32,
        channels: usize,
        data: &[f32],
    ) -> Error {
        let Some(write) = self.driver.write else {
            return OK;
        };

        // SAFETY: `data` is a live slice of at least
        // `width * height * channels` floats, which the caller
        // guarantees, and the driver is told the entry size in bytes.
        unsafe {
            write(
                self.handle,
                xmin,
                xmin + width,
                ymin,
                ymin + height,
                (channels * std::mem::size_of::<f32>()) as c_int,
                data.as_ptr() as *const c_uchar,
            )
        }
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        if let Some(close) = self.driver.close {
            // SAFETY: the handle came from this driver's `open` and has
            // not been closed.
            unsafe {
                close(self.handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static OPENED: AtomicUsize = AtomicUsize::new(0);
    static PIXELS: AtomicUsize = AtomicUsize::new(0);
    static CLOSED: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn open(
        image: *mut ImageHandle,
        _driver: *const c_char,
        _file: *const c_char,
        _width: c_int,
        _height: c_int,
        _param_count: c_int,
        _params: *const UserParameter,
        format_count: c_int,
        _formats: *mut DevFormat,
        _flags: *mut FlagStuff,
    ) -> Error {
        OPENED.fetch_add(format_count as usize, Ordering::SeqCst);
        // A handle of our own, so `close` gets something back.
        unsafe { *image = 1 as ImageHandle };
        OK
    }

    unsafe extern "C" fn write(
        _image: ImageHandle,
        xmin: c_int,
        xmax_plus_one: c_int,
        ymin: c_int,
        ymax_plus_one: c_int,
        _entrysize: c_int,
        _data: *const c_uchar,
    ) -> Error {
        let count = (xmax_plus_one - xmin).max(0) as usize
            * (ymax_plus_one - ymin).max(0) as usize;
        PIXELS.fetch_add(count, Ordering::SeqCst);
        OK
    }

    unsafe extern "C" fn close(_image: ImageHandle) -> Error {
        CLOSED.fetch_add(1, Ordering::SeqCst);
        OK
    }

    fn fake() -> Driver {
        Driver {
            open: Some(open),
            write: Some(write),
            close: Some(close),
            query: None,
        }
    }

    /// **A driver an application registered is found by name.**
    ///
    /// Which is the whole mechanism: an `outputdriver` names a driver
    /// and the renderer has to find it. Nothing did, so no viewport in
    /// any application received a pixel.
    #[test]
    fn a_registered_driver_is_found_by_name() {
        register("test-lookup", fake());
        assert!(lookup("test-lookup").is_some());
        assert!(registered().iter().any(|name| name == "test-lookup"));

        // A null table unregisters, which the interface documents.
        unregister("test-lookup");
        assert!(lookup("test-lookup").is_none());
    }

    /// **Open, write, close, in that order and with the geometry the
    /// interface specifies.**
    ///
    /// `xmax` and `ymax` are one past the end, which is the opposite
    /// of how MoonRay describes a bucket -- getting it wrong loses the
    /// last row and column of every rectangle, visible only as a grid
    /// of seams.
    #[test]
    fn an_image_opens_writes_and_closes() {
        OPENED.store(0, Ordering::SeqCst);
        PIXELS.store(0, Ordering::SeqCst);
        CLOSED.store(0, Ordering::SeqCst);

        let channels =
            vec!["Ci.R".to_owned(), "Ci.G".to_owned(), "Ci.B".to_owned()];
        let image = Image::open(fake(), "test", "beauty", 4, 2, &channels)
            .expect("the driver opened");

        // The channel count reaches the driver, because a driver that
        // is told the wrong one reads past the end of every pixel.
        assert_eq!(OPENED.load(Ordering::SeqCst), 3);

        let pixels = vec![0.5f32; 4 * 2 * 3];
        assert_eq!(image.write(0, 0, 4, 2, 3, &pixels), OK);
        assert_eq!(
            PIXELS.load(Ordering::SeqCst),
            8,
            "four by two pixels, with the ends exclusive"
        );

        drop(image);
        assert_eq!(CLOSED.load(Ordering::SeqCst), 1, "closed exactly once");
    }
}
