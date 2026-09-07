/// The process-wide OSL `ShadingSystem`, and the closures it knows.
///
/// One per process rather than one per material: a `ShadingSystem`
/// owns the JIT, the shader cache and the closure registry, and a
/// second one would compile every shader again. `ShaderGroup`s are
/// per material and cheap.

#pragma once

#include <OSL/oslexec.h>
#include <OSL/oslclosure.h>
#include <OSL/rendererservices.h>

#include <string>

namespace moonray {
namespace shading {
class State;
class Xform;
} // namespace shading
} // namespace moonray

namespace nsi_moonray {

/// What the renderer knows about the point being shaded, for OSL to
/// ask back.
///
/// Reached through `ShaderGlobals::renderstate`, which is the `void*`
/// OSL sets aside for exactly this. Without it every `transform()` in
/// a shader is an identity, which is not an error anywhere -- it
/// renders a plausible picture of the wrong coordinate system.
struct ShadingPoint {
    /// MoonRay's transforms for this shader, built once in `update()`.
    const moonray::shading::Xform* xform;
    /// The point, which is what resolves *object* space: an instanced
    /// prototype's object transform is per shading point, not per
    /// material.
    const moonray::shading::State* state;
};


/// The closures this material understands.
///
/// Ids rather than names at the walk, because a closure component
/// carries its id and OSL's `register_closure` is what binds the two.
/// The order is the registration order and nothing else depends on it.
enum ClosureId {
    CLOSURE_EMISSION,
    CLOSURE_BACKGROUND,
    CLOSURE_DIFFUSE,
    CLOSURE_OREN_NAYAR,
    CLOSURE_TRANSLUCENT,
    CLOSURE_REFLECTION,
    CLOSURE_REFRACTION,
    CLOSURE_TRANSPARENT,
    CLOSURE_MICROFACET,
    CLOSURE_COUNT,
};

/// The parameter blocks OSL writes behind each closure component.
///
/// Layout is the contract: `register_closure` describes it with
/// `CLOSURE_*_PARAM(struct, field)` and OSL writes into that shape, so
/// these declarations and the registration have to agree field for
/// field. `label` is last on every one that has it because it is a
/// keyword parameter.
struct EmptyParams {};

struct DiffuseParams {
    OSL::Vec3 N;
    OSL::ustring label;
};

struct OrenNayarParams {
    OSL::Vec3 N;
    float sigma;
    OSL::ustring label;
};

struct ReflectionParams {
    OSL::Vec3 N;
    float eta;
    OSL::ustring label;
};

struct RefractionParams {
    OSL::Vec3 N;
    float eta;
    OSL::ustring label;
};

struct MicrofacetParams {
    OSL::ustring dist;
    OSL::Vec3 N;
    OSL::Vec3 U;
    float xalpha;
    float yalpha;
    float eta;
    int refract;
    OSL::ustring label;
};

/// The `ShadingSystem` every `Osl` material shares.
///
/// Built on first use and never torn down: OSL's JIT state outlives
/// any one render, and a `ShadingSystem` destroyed while a
/// `ShaderGroup` is alive is undefined.
OSL::ShadingSystem& shading_system();

/// Point the shared system at a place to find `.oso` files.
///
/// Additive, and idempotent per path: several ɴsɪ shaders may name
/// different directories and all of them have to work.
void add_search_path(const std::string& path);

} // namespace nsi_moonray
