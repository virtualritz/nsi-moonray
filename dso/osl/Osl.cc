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
label_index(const OSL::ustring& label)
{
    static const char* const known[] = {
        "diffuse", "specular", "transmission", "subsurface",
        "sheen",   "coat",     "emission",     "hair",
    };

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
distribution(const OSL::ustring& name)
{
    return name == "beckmann" ? ispc::MICROFACET_DISTRIBUTION_BECKMANN
                              : ispc::MICROFACET_DISTRIBUTION_GGX;
}

/// What one walk of a closure tree accumulates.
struct Walk {
    BsdfBuilder& bsdf;
    scene_rdl2::math::Color emission = scene_rdl2::math::sBlack;
    /// Closure ids seen that have no MoonRay lobe. Reported once per
    /// material rather than per shading point, which would be one line
    /// per pixel.
    unsigned unmapped = 0;
};

void
add_microfacet(Walk& walk, const MicrofacetParams& params,
               const scene_rdl2::math::Color& weight)
{
    const scene_rdl2::math::Vec3f normal = to_vec3(params.N);
    // OSL's alpha is a roughness; MoonRay's `roughness` is the same
    // quantity for its GGX and Beckmann lobes.
    const float roughness = params.xalpha;
    const int label = label_index(params.label);

    if (params.refract) {
        const MicrofacetIsotropicBTDF btdf(
            normal, params.eta, roughness, distribution(params.dist),
            ispc::MICROFACET_GEOMETRIC_SMITH, weight, 0.0f);
        walk.bsdf.addMicrofacetIsotropicBTDF(
            btdf, 1.0f, ispc::BSDFBUILDER_PHYSICAL, label);
        return;
    }

    if (is_grey(weight)) {
        // A dielectric: the Fresnel term is the colour, and the weight
        // scales it.
        const MicrofacetIsotropicBRDF brdf(
            normal, params.eta, roughness, distribution(params.dist),
            ispc::MICROFACET_GEOMETRIC_SMITH);
        walk.bsdf.addMicrofacetIsotropicBRDF(
            brdf, weight.r, ispc::BSDFBUILDER_PHYSICAL, label);
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
                                         ispc::BSDFBUILDER_PHYSICAL, label);
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
                                      ispc::BSDFBUILDER_PHYSICAL, label);
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
        walk.emission = walk.emission + total;
        return;

    case CLOSURE_DIFFUSE: {
        const auto* params = component->as<DiffuseParams>();
        const LambertianBRDF brdf(to_vec3(params->N), total);
        walk.bsdf.addLambertianBRDF(brdf, 1.0f, ispc::BSDFBUILDER_PHYSICAL,
                                    label_index(params->label));
        return;
    }

    case CLOSURE_TRANSLUCENT: {
        const auto* params = component->as<DiffuseParams>();
        // OSL's `translucent` diffuses on the far side, which is
        // MoonRay's Lambertian BTDF. Its normal points the other way.
        const LambertianBTDF btdf(-to_vec3(params->N), total);
        walk.bsdf.addLambertianBTDF(btdf, 1.0f, ispc::BSDFBUILDER_PHYSICAL,
                                    label_index(params->label));
        return;
    }

    case CLOSURE_OREN_NAYAR: {
        const auto* params = component->as<OrenNayarParams>();
        const OrenNayarBRDF brdf(to_vec3(params->N), total, params->sigma);
        walk.bsdf.addOrenNayarBRDF(brdf, 1.0f, ispc::BSDFBUILDER_PHYSICAL,
                                   label_index(params->label));
        return;
    }

    case CLOSURE_REFLECTION: {
        const auto* params = component->as<ReflectionParams>();
        const scene_rdl2::math::Vec3f normal = to_vec3(params->N);
        const int label = label_index(params->label);
        if (is_grey(total)) {
            const MirrorBRDF brdf(normal, params->eta);
            walk.bsdf.addMirrorBRDF(brdf, total.r,
                                    ispc::BSDFBUILDER_PHYSICAL, label);
        } else {
            const MirrorBRDF brdf(total, total, normal);
            walk.bsdf.addMirrorBRDF(brdf, 1.0f, ispc::BSDFBUILDER_PHYSICAL,
                                    label);
        }
        return;
    }

    case CLOSURE_REFRACTION: {
        const auto* params = component->as<RefractionParams>();
        const MirrorBTDF btdf(to_vec3(params->N), params->eta, total, 0.0f);
        walk.bsdf.addMirrorBTDF(btdf, 1.0f, ispc::BSDFBUILDER_PHYSICAL,
                                label_index(params->label));
        return;
    }

    case CLOSURE_MICROFACET:
        add_microfacet(walk, *component->as<MicrofacetParams>(), total);
        return;

    case CLOSURE_SUBSURFACE: {
        const auto* params = component->as<SubsurfaceParams>();
        add_subsurface(walk, to_vec3(params->N), total,
                       to_color(params->mfp),
                       label_index(params->label));
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
        walk.bsdf.addOrenNayarBRDF(brdf, 1.0f, ispc::BSDFBUILDER_PHYSICAL,
                                   label_index(params->label));
        return;
    }

    case CLOSURE_MX_DIELECTRIC: {
        const auto* params = component->as<MxDielectricParams>();
        const scene_rdl2::math::Color transmission =
            total * to_color(params->transmission_tint);
        const int label = label_index(params->label);

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
            bsdf, 1.0f, ispc::BSDFBUILDER_PHYSICAL, label, label);
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
            ispc::BSDFBUILDER_PHYSICAL,
            label_index(params->label));
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
            brdf, 1.0f, ispc::BSDFBUILDER_PHYSICAL,
            label_index(params->label));
        return;
    }

    case CLOSURE_MX_TRANSLUCENT: {
        const auto* params = component->as<MxTranslucentParams>();
        const LambertianBTDF btdf(-to_vec3(params->N),
                                  total * to_color(params->albedo));
        walk.bsdf.addLambertianBTDF(btdf, 1.0f, ispc::BSDFBUILDER_PHYSICAL,
                                    label_index(params->label));
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
                       label_index(params->label));
        return;
    }

    case CLOSURE_MX_SHEEN: {
        const auto* params = component->as<MxSheenParams>();
        const VelvetBRDF brdf(to_vec3(params->N), params->roughness,
                              total * to_color(params->albedo), true);
        walk.bsdf.addVelvetBRDF(brdf, 1.0f, ispc::BSDFBUILDER_PHYSICAL,
                                label_index(params->label));
        return;
    }

    case CLOSURE_MX_UNIFORM_EDF: {
        const auto* params = component->as<MxUniformEdfParams>();
        walk.emission = walk.emission + total * to_color(params->emittance);
        return;
    }

    case CLOSURE_MX_LAYER: {
        // Two closures rather than parameters. `BsdfBuilder` layers by
        // the order lobes arrive, so the top goes first -- which is
        // what `BSDFBUILDER_PHYSICAL` then conserves energy across.
        const auto* params = component->as<MxLayerParams>();
        walk_closure(walk, params->top, weight);
        walk_closure(walk, params->base, weight);
        return;
    }

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

/// What a closure tree asks to pass straight through.
///
/// Its own walk rather than a field on `Walk`: presence is evaluated
/// on MoonRay's own function, with no `BsdfBuilder` in hand, so the
/// lobe-building walk cannot be reused. This one visits the same tree
/// and sums only `transparent()`.
scene_rdl2::math::Color
transparency(const OSL::ClosureColor* closure,
             const scene_rdl2::math::Color& weight)
{
    if (closure == nullptr) {
        return scene_rdl2::math::sBlack;
    }

    switch (closure->id) {
    case OSL::ClosureColor::MUL: {
        const auto* mul = closure->as_mul();
        return transparency(mul->closure, weight * to_color(mul->weight));
    }
    case OSL::ClosureColor::ADD: {
        const auto* add = closure->as_add();
        return transparency(add->closureA, weight)
             + transparency(add->closureB, weight);
    }
    default: {
        const auto* component = closure->as_comp();
        if (component->id == CLOSURE_TRANSPARENT
            || component->id == CLOSURE_MX_TRANSPARENT) {
            return weight * to_color(component->w);
        }
        return scene_rdl2::math::sBlack;
    }
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

/// Run the group at one shading point, and hand back its `Ci`.
///
/// Shared by shading and presence, which are two evaluations of the
/// same network -- MoonRay asks for presence on its own function,
/// before shading, so there is nowhere to answer both at once.
namespace {

const OSL::ClosureColor*
execute(const OSL::ShaderGroupRef& group, const Xform* xform,
        const Attributes& attributes, const State& state)
{
    OSL::ShadingSystem& shading = shading_system();

    // One context per thread, kept for the life of the thread: getting
    // one is not free, and shading is the inner loop.
    thread_local OSL::PerThreadInfo* threadInfo =
        shading.create_thread_info();
    thread_local OSL::ShadingContext* context =
        shading.get_context(threadInfo);

    const scene_rdl2::math::Vec3f& position = state.getP();
    const scene_rdl2::math::Vec3f& normal = state.getN();
    const scene_rdl2::math::Vec3f& geometric = state.getNg();
    const scene_rdl2::math::Vec3f& outgoing = state.getWo();
    const scene_rdl2::math::Vec2f& st = state.getSt();
    const scene_rdl2::math::Vec3f& dPds = state.getdPds();
    const scene_rdl2::math::Vec3f& dPdt = state.getdPdt();

    OSL::ShaderGlobals globals = {};
    globals.P = OSL::Vec3(position.x, position.y, position.z);
    globals.N = OSL::Vec3(normal.x, normal.y, normal.z);
    globals.Ng = OSL::Vec3(geometric.x, geometric.y, geometric.z);
    // OSL's `I` is the direction the ray *travelled*, which is the
    // opposite of MoonRay's `wo`, the direction back towards the
    // viewer.
    globals.I = OSL::Vec3(-outgoing.x, -outgoing.y, -outgoing.z);
    globals.u = st.x;
    globals.v = st.y;
    globals.dPdu = OSL::Vec3(dPds.x, dPds.y, dPds.z);
    globals.dPdv = OSL::Vec3(dPdt.x, dPdt.y, dPdt.z);
    globals.surfacearea = 1.0f;
    globals.backfacing = state.isEntering() ? 0 : 1;
    globals.flipHandedness = 0;
    globals.raytype = 1;

    // What `RendererServices` asks back through. Both handles are
    // needed: the `Xform` carries the scene's spaces, and the `State`
    // is what resolves *object* space, which for an instanced
    // prototype is per shading point rather than per material.
    const ShadingPoint point { xform, &state, &attributes };
    globals.renderstate = const_cast<ShadingPoint*>(&point);
    // OSL reaches object and shader space through these, and both
    // land on the same resolution as the named spaces.
    globals.object2common =
        reinterpret_cast<OSL::TransformationPtr>(&point);
    globals.shader2common =
        reinterpret_cast<OSL::TransformationPtr>(&point);

    shading.execute(context, *group, globals);
    return globals.Ci;
}

} // namespace

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

    Walk walk { bsdfBuilder };
    walk_closure(walk,
                 execute(me->mGroup, me->mXform.get(), me->mAttributes,
                         state),
                 scene_rdl2::math::sWhite);

    if (!isBlack(walk.emission)) {
        bsdfBuilder.addEmission(walk.emission);
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
