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

#include <scene_rdl2/common/math/Color.h>

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
        OSL::ustringhash name;
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
    // 3Delight's extensions, which every shader it ships uses.
    CLOSURE_DL_LAYER,
    CLOSURE_DL_OUTPUT_VARIABLE,
    CLOSURE_DL_OUTPUT_CONSTANT,
    CLOSURE_DL_OCCLUSION,
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
    OSL::ustringhash label;
};

struct OrenNayarParams {
    OSL::Vec3 N;
    float sigma;
    OSL::ustringhash label;
};

struct ReflectionParams {
    OSL::Vec3 N;
    float eta;
    OSL::ustringhash label;
};

struct RefractionParams {
    OSL::Vec3 N;
    float eta;
    OSL::ustringhash label;
};

/// `microfacet(string distribution, normal N, vector U, float xalpha,
/// float yalpha, float eta, int refract)`, plus 3Delight's keywords.
///
/// The seven formals are OSL's; `stdosl.h`'s five-argument overload is
/// a wrapper that calls this one, so only this shape reaches a
/// renderer.
///
/// **`realeta` and `complexeta` are how 3Delight spells a conductor.**
/// Its documentation: "the pair (realeta, complexeta) replaces the eta
/// parameter", real and imaginary parts of the base layer's index of
/// refraction -- which is exactly what MoonRay's conductor constructor
/// takes. Without them a 3Delight metal reaches the walk as a plain
/// coloured specular and renders as the wrong metal.
/// **A closure's string parameter is a hash, not a pointer.**
///
/// OSL 1.15 stores `ustringhash` in the parameter block a closure
/// registration describes -- eight bytes of hash where a `ustring`
/// would have held eight bytes of pointer into OpenImageIO's intern
/// table. Declaring the field as `ustring` and reading it is a
/// dereference of `0x5da35be1c5d8d973`, which is a `SIGBUS` inside
/// whatever touched it first.
///
/// It is worse where it does *not* crash. Comparing two such
/// pointer-shaped hashes succeeds as a comparison and fails as an
/// answer, so `microfacet("beckmann", ...)` quietly rendered as GGX.
///
/// Every string field in the structs below is therefore a
/// `ustringhash`, and this is how one is read.
inline OIIO::ustring
text(const OSL::ustringhash& hash)
{
    return OIIO::ustring::from_hash(hash.hash());
}

struct MicrofacetParams {
    OSL::ustringhash dist;
    OSL::Vec3 N;
    OSL::Vec3 U;
    float xalpha;
    float yalpha;
    float eta;
    int refract;
    OSL::Color3 realeta;
    OSL::Color3 complexeta;
    OSL::ustringhash label;
};

/// `subsurface(float eta, float g, color mfp, color albedo)`.
///
/// **Four formals, not five.** There is no `N` among them -- OSL's own
/// `stdosl.h` says so, and 3Delight passes the normal as the keyword
/// `"N"`. A fifth formal here shifted every keyword argument by one, so
/// OSL read a *value* where it expected a key and called `strcmp` on
/// whatever that symbol held. It segfaulted inside `optimize_group`,
/// before a single pixel, on the first real-world shader that used the
/// closure.
struct SubsurfaceParams {
    float eta;
    float g;
    OSL::Color3 mfp;
    OSL::Color3 albedo;
    /// The keyword `"N"`, zero when the shader did not pass one -- OSL
    /// zeroes the parameter block for a closure with no prepare
    /// function, so zero is "unset" and the shading normal stands in.
    OSL::Vec3 N;
    OSL::ustringhash label;
};

/// 3Delight's `layer_closures(closure top, closure bottom, color
/// top_mask)`.
///
/// Not part of OSL: `3delightosl.h` declares it, and every shader
/// 3Delight ships builds its result with it. Two closures rather than
/// parameters, like MaterialX's `layer`.
struct DlLayerParams {
    OSL::ClosureColor* top;
    OSL::ClosureColor* bottom;
    OSL::Color3 top_mask;
};

/// 3Delight's `outputvariable(string name, closure color value)`.
///
/// An AOV wrapper: the closure inside is what shades, and the name is
/// what an ɴsɪ output layer with `variablesource "shader"` asks for.
struct DlOutputVariableParams {
    OSL::ustringhash name;
    OSL::ClosureColor* value;
};

/// 3Delight's `outputconstant(string name)` and `occlusion(normal N)`.
struct DlOutputConstantParams {
    OSL::ustringhash name;
};

struct DlOcclusionParams {
    OSL::Vec3 N;
};

/// MaterialX's diffuse closures: `oren_nayar_diffuse_bsdf` and
/// `burley_diffuse_bsdf` declare the same three.
struct MxDiffuseParams {
    OSL::Vec3 N;
    OSL::Color3 albedo;
    float roughness;
    OSL::ustringhash label;
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
    OSL::ustringhash distribution;
    float thinfilm_thickness;
    float thinfilm_ior;
    OSL::ustringhash label;
};

struct MxConductorParams {
    OSL::Vec3 N;
    OSL::Vec3 U;
    float roughness_x;
    float roughness_y;
    OSL::Color3 ior;
    OSL::Color3 extinction;
    OSL::ustringhash distribution;
    float thinfilm_thickness;
    float thinfilm_ior;
    OSL::ustringhash label;
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
    OSL::ustringhash distribution;
    float thinfilm_thickness;
    float thinfilm_ior;
    OSL::ustringhash label;
};

struct MxTranslucentParams {
    OSL::Vec3 N;
    OSL::Color3 albedo;
    OSL::ustringhash label;
};

struct MxSubsurfaceParams {
    OSL::Vec3 N;
    OSL::Color3 albedo;
    float transmission_depth;
    OSL::Color3 transmission_color;
    float anisotropy;
    OSL::ustringhash label;
};

struct MxSheenParams {
    OSL::Vec3 N;
    OSL::Color3 albedo;
    float roughness;
    OSL::ustringhash label;
};

struct MxUniformEdfParams {
    OSL::Color3 emittance;
    OSL::ustringhash label;
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

/// Run one shader group at one shading point, and hand back its `Ci`.
///
/// Shared by everything that runs a surface network: the material's
/// shading, its presence -- MoonRay asks for that on its own function,
/// before shading, so there is nowhere to answer both at once -- and
/// the `OslMap` a `MeshLight` samples.
const OSL::ClosureColor* execute(const OSL::ShaderGroupRef& group,
                                 const moonray::shading::Xform* xform,
                                 const Attributes& attributes,
                                 const moonray::shading::State& state);

/// What a closure tree emits, weighted.
///
/// Its own walk rather than a step inside the lobe walk, because two
/// things ask: the `Osl` material, which hands it to
/// `BsdfBuilder::addEmission`, and `OslMap`, which is what a
/// `MeshLight` samples to find the radiance of a point on its mesh.
/// One implementation, so a light and the surface it is cannot
/// disagree about how bright the surface is.
scene_rdl2::math::Color emission_of(const OSL::ClosureColor* closure,
                                    const scene_rdl2::math::Color& weight);

/// What a closure tree asks to pass straight through.
///
/// `transparent()` has no MoonRay lobe -- straight-through
/// transmission is *presence* there -- so it is summed on its own walk
/// and read by `Osl::presence`.
scene_rdl2::math::Color transparency(const OSL::ClosureColor* closure,
                                     const scene_rdl2::math::Color& weight);

/// Point the shared system at a place to find `.oso` files.
///
/// Additive, and idempotent per path: several ɴsɪ shaders may name
/// different directories and all of them have to work.
void add_search_path(const std::string& path);

} // namespace nsi_moonray
