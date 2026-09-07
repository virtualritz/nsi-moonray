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
#include <vector>

namespace moonray {
namespace shading {
class State;
class Xform;
} // namespace shading
} // namespace moonray

namespace nsi_moonray {

class Attributes;

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
    /// The primitive attributes this material asked MoonRay for,
    /// resolved once in `update()`.
    ///
    /// Resolved there rather than here because `TypedAttributeKey`'s
    /// name lookup takes a lock, and `getattribute()` in a shader is
    /// the inner loop.
    const Attributes* attributes;
};

/// The primitive attributes one material's shader group reads.
///
/// Which ones is not guesswork: OSL's optimizer reports the name, the
/// scope and the type of every `getattribute()` the group makes, and
/// `Attributes::of` asks it. What MoonRay wants back is an
/// `AttributeKey` per name, which is why they are resolved once rather
/// than at every shading point.
class Attributes {
public:
    /// The attributes a group reads, or nothing if it reads none.
    static Attributes of(const OSL::ShaderGroupRef& group);

    /// The MoonRay keys, for `Shader::mOptionalAttributes`.
    const std::vector<int>& keys() const { return mKeys; }

    /// Read one, into the memory OSL handed over.
    ///
    /// False when the name is not one this group declared, when the
    /// geometry does not carry it, or when the types do not match --
    /// in every case OSL leaves the shader's own default in place,
    /// which is what `getattribute()` returning 0 means.
    bool read(const moonray::shading::State& state, OSL::ustringhash name,
              OSL::TypeDesc type, void* value) const;

private:
    struct Entry {
        OSL::ustring name;
        OSL::TypeDesc type;
        int key;
    };

    std::vector<Entry> mEntries;
    std::vector<int> mKeys;
};


/// The closures this material understands.
///
/// Ids rather than names at the walk, because a closure component
/// carries its id and OSL's `register_closure` is what binds the two.
/// The order is the registration order and nothing else depends on it.
enum ClosureId {
    // OSL's originals.
    CLOSURE_EMISSION,
    CLOSURE_BACKGROUND,
    CLOSURE_DIFFUSE,
    CLOSURE_OREN_NAYAR,
    CLOSURE_TRANSLUCENT,
    CLOSURE_REFLECTION,
    CLOSURE_REFRACTION,
    CLOSURE_TRANSPARENT,
    CLOSURE_MICROFACET,
    CLOSURE_SUBSURFACE,
    // MaterialX's, which is what a shader written this decade emits.
    CLOSURE_MX_OREN_NAYAR,
    CLOSURE_MX_BURLEY,
    CLOSURE_MX_DIELECTRIC,
    CLOSURE_MX_CONDUCTOR,
    CLOSURE_MX_GENERALIZED_SCHLICK,
    CLOSURE_MX_TRANSLUCENT,
    CLOSURE_MX_TRANSPARENT,
    CLOSURE_MX_SUBSURFACE,
    CLOSURE_MX_SHEEN,
    CLOSURE_MX_UNIFORM_EDF,
    CLOSURE_MX_LAYER,
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

struct SubsurfaceParams {
    OSL::Vec3 N;
    float eta;
    float g;
    OSL::Color3 mfp;
    OSL::Color3 albedo;
    OSL::ustring label;
};

/// MaterialX's diffuse closures: `oren_nayar_diffuse_bsdf` and
/// `burley_diffuse_bsdf` declare the same three.
struct MxDiffuseParams {
    OSL::Vec3 N;
    OSL::Color3 albedo;
    float roughness;
    OSL::ustring label;
};

/// What every MaterialX microfacet closure starts with. The order is
/// the contract: OSL writes into this layout, so it has to match the
/// registration field for field.
struct MxDielectricParams {
    OSL::Vec3 N;
    OSL::Vec3 U;
    OSL::Color3 reflection_tint;
    OSL::Color3 transmission_tint;
    float roughness_x;
    float roughness_y;
    float ior;
    OSL::ustring distribution;
    float thinfilm_thickness;
    float thinfilm_ior;
    OSL::ustring label;
};

struct MxConductorParams {
    OSL::Vec3 N;
    OSL::Vec3 U;
    float roughness_x;
    float roughness_y;
    OSL::Color3 ior;
    OSL::Color3 extinction;
    OSL::ustring distribution;
    float thinfilm_thickness;
    float thinfilm_ior;
    OSL::ustring label;
};

struct MxGeneralizedSchlickParams {
    OSL::Vec3 N;
    OSL::Vec3 U;
    OSL::Color3 reflection_tint;
    OSL::Color3 transmission_tint;
    float roughness_x;
    float roughness_y;
    OSL::Color3 f0;
    OSL::Color3 f90;
    float exponent;
    OSL::ustring distribution;
    float thinfilm_thickness;
    float thinfilm_ior;
    OSL::ustring label;
};

struct MxTranslucentParams {
    OSL::Vec3 N;
    OSL::Color3 albedo;
    OSL::ustring label;
};

struct MxSubsurfaceParams {
    OSL::Vec3 N;
    OSL::Color3 albedo;
    float transmission_depth;
    OSL::Color3 transmission_color;
    float anisotropy;
    OSL::ustring label;
};

struct MxSheenParams {
    OSL::Vec3 N;
    OSL::Color3 albedo;
    float roughness;
    OSL::ustring label;
};

struct MxUniformEdfParams {
    OSL::Color3 emittance;
    OSL::ustring label;
};

/// `layer(top, base)`: two closures rather than parameters, which is
/// why the walk descends into them instead of reading a struct.
struct MxLayerParams {
    OSL::ClosureColor* top;
    OSL::ClosureColor* base;
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
