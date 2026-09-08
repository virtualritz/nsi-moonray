/// An OSL surface, as a MoonRay `Material`.
///
/// ɴsɪ *is* OSL: a `shader` node names a compiled `.oso` and carries
/// that shader's parameters, and section 4.5 of the specification says
/// a light is geometry whose surface shader produces an `emission()`
/// closure. So this is not a translation of ɴsɪ's shading model --
/// running it *is* the mapping.
///
/// # Scalar only, deliberately
///
/// `mShadeFuncv` is left null. OSL's `ShadingSystem` shades one point
/// at a time and has no ISPC function to offer, and MoonRay's
/// `Material::shadev` null-checks the pointer and silently does
/// nothing -- so in vectorized mode, which is the default, a material
/// like this renders **black** with no diagnostic. Measured:
/// `tools/scalar-material`, `specs/003-osl/research.md` O1, and
/// `upstream/moonray-scalar-material-renders-black.md`.
///
/// Whoever builds the render has to force scalar execution. This
/// backend's shim does; a scene handed to `moonray` by hand needs
/// `-exec_mode scalar`.

#define NSI_MOONRAY_OSL_ROOT rdl2::Material
#define NSI_MOONRAY_OSL_LABELS
#include "attributes.cc"
#include "shading_system.h"

#include <moonray/rendering/shading/MaterialApi.h>

#include <cstdlib>
#include <memory>
#include <string>

using namespace moonray::shading;
using namespace nsi_moonray;

namespace {

/// The label a lobe carries, as MoonRay's integer.
///
/// The vocabulary is `labels[]` in `attributes.cc` and the indices are
/// one-based, because zero means "no label" to `BsdfBuilder`. A label
/// outside the vocabulary becomes zero -- the lobe still shades, it
/// just cannot be named by a material AOV or an LPE.
int
label_index(const OSL::ustringhash& label_hash)
{
    static const char* const known[] = {
        "diffuse", "specular", "transmission", "subsurface",
        "sheen",   "coat",     "emission",     "hair",
    };

    const OIIO::ustring label = text(label_hash);
    if (label.empty()) {
        return 0;
    }
    for (int i = 0; i < static_cast<int>(sizeof(known) / sizeof(*known)); ++i) {
        if (label == known[i]) {
            return i + 1;
        }
    }
    return 0;
}

/// A 3Delight AOV name, as a lobe label.
///
/// `outputvariable("reflection", ...)` is 3Delight's way of naming the
/// specular part of a shader, and MoonRay's lobe labels are the same
/// idea under different words -- so the two are reconciled here rather
/// than left as two vocabularies for one thing. The names are
/// 3Delight's own, read off the shaders it ships.
///
/// A name with no lobe behind it -- `"albedo"`, which is data rather
/// than scattering -- leaves the label alone.
int
aov_label(const OSL::ustringhash& name_hash)
{
    static const struct {
        const char* aov;
        const char* label;
    } known[] = {
        { "diffuse", "diffuse" },   { "reflection", "specular" },
        { "refraction", "transmission" },
        { "subsurface", "subsurface" }, { "sheen", "sheen" },
        { "coating", "coat" },      { "incandescence", "emission" },
        { "hair", "hair" },
    };

    const OIIO::ustring name = text(name_hash);
    for (const auto& entry : known) {
        if (name == entry.aov) {
            return label_index(OSL::ustringhash(entry.label));
        }
    }
    return 0;
}

scene_rdl2::math::Color
to_color(const OSL::Color3& color)
{
    return scene_rdl2::math::Color(color.x, color.y, color.z);
}

scene_rdl2::math::Vec3f
to_vec3(const OSL::Vec3& vector)
{
    return scene_rdl2::math::Vec3f(vector.x, vector.y, vector.z);
}

/// Whether a weight is grey, and so expressible as MoonRay's scalar.
bool
is_grey(const scene_rdl2::math::Color& weight)
{
    return scene_rdl2::math::isEqual(weight.r, weight.g)
           && scene_rdl2::math::isEqual(weight.g, weight.b);
}

/// GGX unless the shader said Beckmann. OSL names the distribution as
/// a string; MoonRay as an enum, and it has exactly these two.
ispc::MicrofacetDistribution
distribution(const OSL::ustringhash& name_hash)
{
    return text(name_hash) == "beckmann"
               ? ispc::MICROFACET_DISTRIBUTION_BECKMANN
               : ispc::MICROFACET_DISTRIBUTION_GGX;
}

/// What one walk of a closure tree accumulates.
struct Walk {
    BsdfBuilder& bsdf;
    /// How a lobe added here interacts with the ones around it.
    ///
    /// **OSL's `+` is a sum, not a layering.** `Ci = a + b` says the
    /// two closures add; MoonRay's `BSDFBUILDER_PHYSICAL` says the
    /// first attenuates the second, so a shader written as
    /// `diffuse() + microfacet()` lost whichever came second --
    /// measured: the specular AOV was black, and swapping the two
    /// terms in the shader swapped which one vanished.
    ///
    /// So an `add` node walks its children additively, and *layering*
    /// -- MaterialX's `layer` and 3Delight's `layer_closures`, which
    /// are the closures that mean it -- is what turns the attenuation
    /// on. The flags compose, so a layer inside a layer still layers.
    int behaviour = ispc::BSDFBUILDER_ADDITIVE;
    /// The label a lobe takes when it carries none of its own.
    ///
    /// 3Delight's shaders label nothing directly: they wrap each part
    /// of the surface in `outputvariable("reflection", ...)` and so on,
    /// which is the same intent one level up. So the wrapper sets this
    /// for the closures inside it.
    int label = 0;
    /// The shading normal, for a closure that carries no normal of its
    /// own. `subsurface` is one: OSL declares no `N` formal for it, and
    /// a shader that passes none means "the surface's".
    scene_rdl2::math::Vec3f normal;
    /// Closure ids seen that have no MoonRay lobe. Reported once per
    /// material rather than per shading point, which would be one line
    /// per pixel.
    unsigned unmapped = 0;
};

/// The label for one lobe: its own if it has one, the walk's otherwise.
int
labelled(const Walk& walk, const OSL::ustringhash& label)
{
    const int own = label_index(label);
    return own != 0 ? own : walk.label;
}

void
add_microfacet(Walk& walk, const MicrofacetParams& params,
               const scene_rdl2::math::Color& weight)
{
    const scene_rdl2::math::Vec3f normal = to_vec3(params.N);
    // OSL's alpha is a roughness; MoonRay's `roughness` is the same
    // quantity for its GGX and Beckmann lobes.
    const float roughness = params.xalpha;
    const int label = labelled(walk, params.label);

    if (params.refract) {
        const MicrofacetIsotropicBTDF btdf(
            normal, params.eta, roughness, distribution(params.dist),
            ispc::MICROFACET_GEOMETRIC_SMITH, weight, 0.0f);
        walk.bsdf.addMicrofacetIsotropicBTDF(
            btdf, 1.0f, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour), label);
        return;
    }

    // A complex index of refraction, which is what a conductor *is*:
    // 3Delight passes it rather than tinting the weight, and MoonRay's
    // conductor constructor takes exactly this pair. Checked before
    // the grey/coloured split because a metal with a white base colour
    // would otherwise take the dielectric path.
    const scene_rdl2::math::Color complex = to_color(params.complexeta);
    if (!isBlack(complex)) {
        const MicrofacetIsotropicBRDF brdf(
            to_color(params.realeta), complex, normal, roughness,
            distribution(params.dist), ispc::MICROFACET_GEOMETRIC_SMITH);
        walk.bsdf.addMicrofacetIsotropicBRDF(
            brdf, scene_rdl2::math::luminance(weight),
            static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour), label);
        return;
    }

    if (is_grey(weight)) {
        // A dielectric: the Fresnel term is the colour, and the weight
        // scales it.
        const MicrofacetIsotropicBRDF brdf(
            normal, params.eta, roughness, distribution(params.dist),
            ispc::MICROFACET_GEOMETRIC_SMITH);
        walk.bsdf.addMicrofacetIsotropicBRDF(
            brdf, weight.r, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour), label);
        return;
    }

    // A coloured specular is a conductor: MoonRay's artist-friendly
    // constructor takes the reflectivity and the edge tint, which is
    // how `UsdPreviewSurface` spells metal too. Folding the colour
    // into a scalar weight instead would render a grey metal.
    const MicrofacetIsotropicBRDF brdf(
        normal, weight, weight, roughness, distribution(params.dist),
        ispc::MICROFACET_GEOMETRIC_SMITH);
    walk.bsdf.addMicrofacetIsotropicBRDF(brdf, 1.0f,
                                         static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour), label);
}

void walk_closure(Walk& walk, const OSL::ClosureColor* closure,
                  const scene_rdl2::math::Color& weight);

/// Subsurface, which MoonRay wants as a scattering radius.
///
/// `RandomWalkSubsurface` needs the material and a normal-evaluation
/// function for its own crease handling; neither is available from a
/// shading point, so it gets nulls and MoonRay falls back to the
/// unattenuated form.
void
add_subsurface(Walk& walk, const scene_rdl2::math::Vec3f& normal,
               const scene_rdl2::math::Color& albedo,
               const scene_rdl2::math::Color& radius, int label)
{
    const RandomWalkSubsurface subsurface(normal, albedo, radius, 1.0f,
                                          false, nullptr, nullptr, nullptr);
    walk.bsdf.addRandomWalkSubsurface(subsurface, 1.0f,
                                      static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour), label);
}

/// Flatten one closure tree into `BsdfBuilder` calls.
///
/// OSL hands back a tree of `add`, `mul` and component nodes and
/// leaves the flattening to the renderer; `BsdfBuilder` takes lobes in
/// order and does its own energy-conserving layering. Weights fold
/// down the tree, which is what makes the two compatible at all.
void
walk_closure(Walk& walk, const OSL::ClosureColor* closure,
             const scene_rdl2::math::Color& weight)
{
    if (closure == nullptr) {
        return;
    }

    switch (closure->id) {
    case OSL::ClosureColor::MUL: {
        const OSL::ClosureMul* mul = closure->as_mul();
        walk_closure(walk, mul->closure, weight * to_color(mul->weight));
        return;
    }
    case OSL::ClosureColor::ADD: {
        const OSL::ClosureAdd* add = closure->as_add();
        walk_closure(walk, add->closureA, weight);
        walk_closure(walk, add->closureB, weight);
        return;
    }
    default:
        break;
    }

    const OSL::ClosureComponent* component = closure->as_comp();
    const scene_rdl2::math::Color total = weight * to_color(component->w);

    switch (component->id) {
    case CLOSURE_EMISSION:
        // Accumulated rather than added straight through: a `Bsdf`
        // carries one self-emission colour, and `BsdfBuilder`'s
        // `addEmission` sums, so either works -- summing here keeps
        // the total available for the report.
        //
        // What this cannot do is make the surface a *light*. MoonRay's
        // self-emission is hit-only: no next-event estimation, no
        // shadow rays. An ɴsɪ emitter that is meant to light the scene
        // becomes a `MeshLight` in the flush instead
        // (`specs/003-osl/research.md` O2, O3).
        return;
        return;

    case CLOSURE_DIFFUSE: {
        const auto* params = component->as<DiffuseParams>();
        const LambertianBRDF brdf(to_vec3(params->N), total);
        walk.bsdf.addLambertianBRDF(brdf, 1.0f, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour),
                                    labelled(walk, params->label));
        return;
    }

    case CLOSURE_TRANSLUCENT: {
        const auto* params = component->as<DiffuseParams>();
        // OSL's `translucent` diffuses on the far side, which is
        // MoonRay's Lambertian BTDF. Its normal points the other way.
        const LambertianBTDF btdf(-to_vec3(params->N), total);
        walk.bsdf.addLambertianBTDF(btdf, 1.0f, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour),
                                    labelled(walk, params->label));
        return;
    }

    case CLOSURE_OREN_NAYAR: {
        const auto* params = component->as<OrenNayarParams>();
        const OrenNayarBRDF brdf(to_vec3(params->N), total, params->sigma);
        walk.bsdf.addOrenNayarBRDF(brdf, 1.0f, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour),
                                   labelled(walk, params->label));
        return;
    }

    case CLOSURE_REFLECTION: {
        const auto* params = component->as<ReflectionParams>();
        const scene_rdl2::math::Vec3f normal = to_vec3(params->N);
        const int label = labelled(walk, params->label);
        if (is_grey(total)) {
            const MirrorBRDF brdf(normal, params->eta);
            walk.bsdf.addMirrorBRDF(brdf, total.r,
                                    static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour), label);
        } else {
            const MirrorBRDF brdf(total, total, normal);
            walk.bsdf.addMirrorBRDF(brdf, 1.0f, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour),
                                    label);
        }
        return;
    }

    case CLOSURE_REFRACTION: {
        const auto* params = component->as<RefractionParams>();
        const MirrorBTDF btdf(to_vec3(params->N), params->eta, total, 0.0f);
        walk.bsdf.addMirrorBTDF(btdf, 1.0f, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour),
                                labelled(walk, params->label));
        return;
    }

    case CLOSURE_MICROFACET:
        add_microfacet(walk, *component->as<MicrofacetParams>(), total);
        return;

    case CLOSURE_SUBSURFACE: {
        const auto* params = component->as<SubsurfaceParams>();
        // `N` is a keyword here, not a formal, and OSL zeroes the
        // parameter block when the shader does not pass one.
        const OSL::Vec3 given = params->N;
        const scene_rdl2::math::Vec3f normal =
            (given.x == 0.0f && given.y == 0.0f && given.z == 0.0f)
                ? walk.normal
                : to_vec3(given);
        add_subsurface(walk, normal, total, to_color(params->mfp),
                       labelled(walk, params->label));
        return;
    }

    // MaterialX, which is what a shader written this decade emits.
    case CLOSURE_MX_OREN_NAYAR:
    case CLOSURE_MX_BURLEY: {
        // Burley is a diffuse with a roughness term, which is the
        // shape MoonRay's Oren-Nayar lobe has. Nearer than Lambert,
        // and reported nowhere because it is a lobe substitution
        // rather than a lost parameter.
        const auto* params = component->as<MxDiffuseParams>();
        const OrenNayarBRDF brdf(to_vec3(params->N),
                                 total * to_color(params->albedo),
                                 params->roughness);
        walk.bsdf.addOrenNayarBRDF(brdf, 1.0f, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour),
                                   labelled(walk, params->label));
        return;
    }

    case CLOSURE_MX_DIELECTRIC: {
        const auto* params = component->as<MxDielectricParams>();
        const scene_rdl2::math::Color transmission =
            total * to_color(params->transmission_tint);
        const int label = labelled(walk, params->label);

        // Reflection and transmission in one lobe when both are
        // wanted, which is what MoonRay's BSDF form is for: it
        // balances the two by Fresnel rather than letting the shader
        // add them and exceed one. The two weights are scalars, so a
        // coloured tint collapses to its Rec. 709 luminance; a grey
        // one — the common case — passes through unchanged.
        const MicrofacetIsotropicBSDF bsdf(
            to_vec3(params->N), params->ior, params->roughness_x,
            distribution(params->distribution),
            ispc::MICROFACET_GEOMETRIC_SMITH, transmission, 0.0f,
            params->ior,
            scene_rdl2::math::luminance(total
                                        * to_color(params->reflection_tint)),
            scene_rdl2::math::luminance(transmission));
        walk.bsdf.addMicrofacetIsotropicBSDF(
            bsdf, 1.0f, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour), label, label);
        return;
    }

    case CLOSURE_MX_CONDUCTOR: {
        const auto* params = component->as<MxConductorParams>();
        // MoonRay's conductor takes the complex index of refraction
        // directly, which is exactly what MaterialX supplies.
        const MicrofacetIsotropicBRDF brdf(
            to_color(params->ior), to_color(params->extinction),
            to_vec3(params->N), params->roughness_x,
            distribution(params->distribution),
            ispc::MICROFACET_GEOMETRIC_SMITH);
        walk.bsdf.addMicrofacetIsotropicBRDF(
            brdf, scene_rdl2::math::luminance(total),
            static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour),
            labelled(walk, params->label));
        return;
    }

    case CLOSURE_MX_GENERALIZED_SCHLICK: {
        const auto* params = component->as<MxGeneralizedSchlickParams>();
        // Schlick's `f0` and `f90` are reflectivity at normal and
        // grazing incidence, which is what MoonRay's artist-friendly
        // conductor constructor calls reflectivity and edge tint. The
        // `exponent` has no counterpart and is not carried.
        const MicrofacetIsotropicBRDF brdf(
            to_vec3(params->N),
            total * to_color(params->f0) * to_color(params->reflection_tint),
            to_color(params->f90), params->roughness_x,
            distribution(params->distribution),
            ispc::MICROFACET_GEOMETRIC_SMITH);
        walk.bsdf.addMicrofacetIsotropicBRDF(
            brdf, 1.0f, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour),
            labelled(walk, params->label));
        return;
    }

    case CLOSURE_MX_TRANSLUCENT: {
        const auto* params = component->as<MxTranslucentParams>();
        const LambertianBTDF btdf(-to_vec3(params->N),
                                  total * to_color(params->albedo));
        walk.bsdf.addLambertianBTDF(btdf, 1.0f, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour),
                                    labelled(walk, params->label));
        return;
    }

    case CLOSURE_MX_SUBSURFACE: {
        const auto* params = component->as<MxSubsurfaceParams>();
        // MaterialX gives a depth and a colour where MoonRay wants a
        // per-channel radius; the depth scales the colour into one.
        add_subsurface(walk, to_vec3(params->N),
                       total * to_color(params->albedo),
                       to_color(params->transmission_color)
                           * params->transmission_depth,
                       labelled(walk, params->label));
        return;
    }

    case CLOSURE_MX_SHEEN: {
        const auto* params = component->as<MxSheenParams>();
        const VelvetBRDF brdf(to_vec3(params->N), params->roughness,
                              total * to_color(params->albedo), true);
        walk.bsdf.addVelvetBRDF(brdf, 1.0f, static_cast<ispc::BsdfBuilderBehavior>(walk.behaviour),
                                labelled(walk, params->label));
        return;
    }

    case CLOSURE_MX_UNIFORM_EDF:
        // MaterialX's emission. See `CLOSURE_EMISSION`.
        return;

    case CLOSURE_MX_LAYER: {
        // Two closures rather than parameters, and the closure that
        // *means* layering -- so this is where the attenuation is
        // turned on. The top goes first, because `BsdfBuilder` layers
        // by the order lobes arrive.
        const auto* params = component->as<MxLayerParams>();
        const int outer = walk.behaviour;
        walk.behaviour = outer | ispc::BSDFBUILDER_OVER_SUBSEQUENT;
        walk_closure(walk, params->top, weight);
        walk.behaviour = outer | ispc::BSDFBUILDER_UNDER_PREVIOUS;
        walk_closure(walk, params->base, weight);
        walk.behaviour = outer;
        return;
    }

    case CLOSURE_DL_LAYER: {
        // 3Delight layers two closures and leaves the energy
        // conservation to the renderer, which is exactly what
        // `BsdfBuilder` does with lobes added in order. So the mask
        // scales the top and the bottom goes in behind it, unscaled --
        // MoonRay reduces it itself.
        const auto* params = component->as<DlLayerParams>();
        const int outer = walk.behaviour;
        walk.behaviour = outer | ispc::BSDFBUILDER_OVER_SUBSEQUENT;
        walk_closure(walk, params->top, total * to_color(params->top_mask));
        walk.behaviour = outer | ispc::BSDFBUILDER_UNDER_PREVIOUS;
        walk_closure(walk, params->bottom, total);
        walk.behaviour = outer;
        return;
    }

    case CLOSURE_DL_OUTPUT_VARIABLE: {
        // An AOV wrapper: the closure inside is what shades. The name
        // is what an ɴsɪ output layer with `variablesource "shader"`
        // asks for, and it is *also* the closest thing 3Delight's
        // shaders have to a lobe label -- so it becomes one where the
        // vocabulary has a match, and is otherwise just passed through.
        const auto* params = component->as<DlOutputVariableParams>();
        const int outer = walk.label;
        const int named = aov_label(params->name);
        if (named != 0) {
            walk.label = named;
        }
        walk_closure(walk, params->value, total);
        walk.label = outer;
        return;
    }

    case CLOSURE_DL_OUTPUT_CONSTANT:
        // A named constant for an AOV. It shades nothing, so there is
        // nothing to lose by ignoring it here.
        return;

    case CLOSURE_DL_OCCLUSION:
        // An ambient-occlusion probe, which MoonRay has no lobe for --
        // it is a render-time query, not a scattering function.
        ++walk.unmapped;
        return;

    case CLOSURE_MX_TRANSPARENT:
    case CLOSURE_TRANSPARENT:
        // Straight-through transmission, which MoonRay takes as
        // *presence* rather than as a lobe -- on its own function,
        // evaluated before shading. `transparency` is the walk that
        // reads it, so there is nothing to do here and nothing lost.
        return;

    case CLOSURE_BACKGROUND:
        // A background is an environment, not a surface. It reaches
        // MoonRay as an `EnvLight` from the ɴsɪ `environment` node, not
        // through a material.
        ++walk.unmapped;
        return;

    default:
        ++walk.unmapped;
        return;
    }
}

} // namespace

RDL2_DSO_CLASS_BEGIN(Osl, scene_rdl2::rdl2::Material)

public:
    Osl(const scene_rdl2::rdl2::SceneClass& sceneClass,
        const std::string& name);

    void update() override;

    static void shade(const scene_rdl2::rdl2::Material* self,
                      moonray::shading::TLState* tls,
                      const State& state,
                      BsdfBuilder& bsdfBuilder);

    static float presence(const scene_rdl2::rdl2::Material* self,
                          moonray::shading::TLState* tls,
                          const State& state);

private:
    OSL::ShaderGroupRef mGroup;
    /// MoonRay's transforms for this shader.
    ///
    /// Built in `update()` as `Xform`'s own documentation asks, and
    /// held: it is what makes `transform("object", P)` in a shader
    /// mean what it says instead of quietly being an identity.
    std::unique_ptr<Xform> mXform;
    /// The primitive attributes this group's shaders read, resolved
    /// once. See `Attributes`.
    Attributes mAttributes;

RDL2_DSO_CLASS_END(Osl)

Osl::Osl(const scene_rdl2::rdl2::SceneClass& sceneClass,
         const std::string& name)
    : Parent(sceneClass, name)
{
    mShadeFunc = Osl::shade;
    // See the note at the top of this file: null, and it costs a
    // correct image in the default execution mode.
    mShadeFuncv = nullptr;
}

void
Osl::update()
{
    mGroup.reset();
    // Default spaces: the shading point's own object, the scene's
    // active camera, the scene's aspect ratio. An ɴsɪ shader has no
    // transform of its own, so there is nothing to override them with.
    mXform = std::make_unique<Xform>(this);

    const std::string& spec = get(attrGroupSpec);
    if (spec.empty()) {
        error("no OSL group specification; this material shades nothing");
        return;
    }

    std::string search = get(attrSearchPath);
    if (search.empty()) {
        if (const char* fromEnvironment = std::getenv("OSL_SHADER_PATH")) {
            search = fromEnvironment;
        }
    }
    add_search_path(search);

    OSL::ShadingSystem& shading = shading_system();
    const std::string& groupName = get(attrGroupName);

    mGroup = shading.ShaderGroupBegin(groupName.empty() ? getName()
                                                        : groupName,
                                      "surface", spec);
    if (!mGroup) {
        error("the OSL group specification did not parse; this material "
              "shades nothing");
        return;
    }
    shading.ShaderGroupEnd(*mGroup);

    // What the shaders read off the geometry, and what MoonRay has to
    // be asked for so an intersection carries it. *Optional* rather
    // than required: a mesh without the attribute should render with
    // the shader's own default, which is what OSL does when
    // `getattribute` returns 0.
    mAttributes = Attributes::of(mGroup);
    mOptionalAttributes = mAttributes.keys();

    // **Presence costs a second run of the network**, so it is only
    // installed for a group that can actually produce one. OSL knows:
    // `closures_needed` is what the optimizer found the group may
    // emit, and `unknown_closures_needed` is its own admission that it
    // could not tell -- in which case the material pays, rather than
    // rendering an opaque surface a shader asked to see through.
    mPresenceFunc = scene_rdl2::rdl2::Material::defaultPresence;
    shading.optimize_group(mGroup.get(), nullptr, true);

    int unknown = 0;
    shading.getattribute(mGroup.get(), "unknown_closures_needed",
                         OSL::TypeDesc::INT, &unknown);
    if (unknown) {
        mPresenceFunc = Osl::presence;
        return;
    }

    int count = 0;
    OSL::ustring* needed = nullptr;
    if (shading.getattribute(mGroup.get(), "num_closures_needed",
                             OSL::TypeDesc::INT, &count)
        && shading.getattribute(mGroup.get(), "closures_needed",
                                OSL::TypeDesc::PTR, &needed)
        && needed != nullptr) {
        for (int i = 0; i < count; ++i) {
            if (needed[i] == "transparent"
                || needed[i] == "transparent_bsdf") {
                mPresenceFunc = Osl::presence;
                return;
            }
        }
    }
}

void
Osl::shade(const scene_rdl2::rdl2::Material* self,
           moonray::shading::TLState* /*tls*/,
           const State& state,
           BsdfBuilder& bsdfBuilder)
{
    const Osl* me = static_cast<const Osl*>(self);
    if (!me->mGroup) {
        return;
    }

    const OSL::ClosureColor* closure =
        execute(me->mGroup, me->mXform.get(), me->mAttributes, state);

    Walk walk { bsdfBuilder, ispc::BSDFBUILDER_ADDITIVE, 0,
                state.getN() };
    walk_closure(walk, closure, scene_rdl2::math::sWhite);

    // Emission on its own walk rather than out of the lobe walk: a
    // `MeshLight`'s `OslMap` asks the same question of the same
    // network, and one implementation is what keeps a light and the
    // surface it is from disagreeing about how bright it is.
    const scene_rdl2::math::Color emission =
        emission_of(closure, scene_rdl2::math::sWhite);
    if (!isBlack(emission)) {
        bsdfBuilder.addEmission(emission);
    }
}

float
Osl::presence(const scene_rdl2::rdl2::Material* self,
              moonray::shading::TLState* /*tls*/,
              const State& state)
{
    const Osl* me = static_cast<const Osl*>(self);
    if (!me->mGroup) {
        return 1.0f;
    }

    // A second run of the whole network, which is why this is
    // installed only for a group OSL says may emit `transparent` --
    // see `update`.
    const scene_rdl2::math::Color through =
        transparency(execute(me->mGroup, me->mXform.get(), me->mAttributes,
                             state),
                     scene_rdl2::math::sWhite);

    // `transparent()` is the fraction that passes straight through, so
    // presence is what is left. A coloured transparency has nowhere to
    // go -- presence is one number -- and collapses to its luminance.
    return scene_rdl2::math::clamp(
        1.0f - scene_rdl2::math::luminance(through), 0.0f, 1.0f);
}
