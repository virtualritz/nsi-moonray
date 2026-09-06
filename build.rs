//! Builds the `scene_rdl2` shim when the `rdl2` feature is on.
//!
//! Off by default, and deliberately: the emitter, the format oracle and
//! the flush are all checked without a renderer present, and that is
//! what lets this crate be worked on from a machine that cannot build
//! MoonRay. Turning the feature on is what asks for `scene_rdl2`.

fn main() {
    println!("cargo::rerun-if-changed=shim/src/scene.cc");
    println!("cargo::rerun-if-changed=shim/include/nsi_moonray_shim.h");
    println!("cargo::rerun-if-env-changed=SCENE_RDL2_ROOT");
    println!("cargo::rerun-if-env-changed=MOONRAY_ROOT");
    println!("cargo::rerun-if-changed=shim/src/render.cc");
    println!("cargo::rustc-check-cfg=cfg(moonray)");

    if std::env::var_os("CARGO_FEATURE_RDL2").is_none() {
        return;
    }

    let root = std::env::var("SCENE_RDL2_ROOT").unwrap_or_else(|_| {
        panic!(
            "the `rdl2` feature needs $SCENE_RDL2_ROOT set to a \
             `scene_rdl2` install prefix -- the one `quickstart.md` \
             builds into"
        )
    });

    let lua = std::env::var("LUA_INCLUDE_DIR")
        .unwrap_or_else(|_| "/usr/include/lua5.3".to_string());

    // MoonRay is a separate ask from `scene_rdl2`: the scene half needs
    // only rdl2, and that is what lets a scene be built and checked on
    // a host that cannot build the renderer.
    let moonray = std::env::var("MOONRAY_ROOT").ok();

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .file("shim/src/scene.cc")
        .include("shim/include")
        .include(format!("{root}/include"))
        // `AsciiReader.h` includes `lua.hpp`, so Lua's headers are
        // needed even to only *write* a scene.
        .include(&lua)
        // Without these `rdl2/Types.h` does not parse at all -- it dies
        // at its first `__cdecl` function typedef. rdl2's own build
        // passes them on the command line, so a consumer has to repeat
        // them. See `quickstart.md`.
        .define("__cdecl", "")
        .define("PLATFORM_UNIX", None)
        .define("PLATFORM_LINUX", None)
        .define("__AVX__", None)
        .flag("-mavx")
        // rdl2's own headers trip these by the hundred -- unused
        // parameters in its virtual defaults, and type-punned pointers
        // in its intrinsics -- and they are not ours to fix. Left on
        // for our own translation unit only would be better; `cc` has
        // no per-header switch, so they are off.
        .warnings(false)
        .flag_if_supported("-Wno-strict-aliasing")
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-deprecated-declarations");

    if let Some(moonray) = &moonray {
        println!("cargo::rustc-cfg=moonray");
        build
            .file("shim/src/render.cc")
            .include(format!("{moonray}/include"));
    }

    build.compile("nsi_moonray_shim");

    println!("cargo::rustc-link-search=native={root}/lib");
    println!("cargo::rustc-link-lib=dylib=scene_rdl2");
    // rdl2 calls `Logger::logFatal`, which the linker does not pull in
    // transitively.
    println!("cargo::rustc-link-lib=dylib=render_logging");
    // So the built artefact finds them without `LD_LIBRARY_PATH`, which
    // a `dlopen`ing host will not have set.
    println!("cargo::rustc-link-arg=-Wl,-rpath,{root}/lib");

    if let Some(moonray) = &moonray {
        println!("cargo::rustc-link-search=native={moonray}/lib");
        // `rendering_rndr` is the renderer; the rest are what it needs
        // and the linker does not pull in transitively.
        for library in [
            "rendering_rndr",
            "rendering_rt",
            "rendering_pbr",
            "rendering_geom",
            "rendering_shading",
            "rendering_mcrt_common",
            "common_grid_util",
            "common_fb_util",
        ] {
            println!("cargo::rustc-link-lib=dylib={library}");
        }
        println!("cargo::rustc-link-arg=-Wl,-rpath,{moonray}/lib");
    }
}
