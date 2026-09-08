// **rdl2's headers first, and it matters.**
// `scene_rdl2/render/util/AtomicFloat.h` *specialises*
// `std::atomic<float>`, and OIIO -- which OSL's headers pull in --
// instantiates it. Whichever comes second loses, with
// "specialization of 'std::atomic<float>' after instantiation" and a
// backtrace that points at neither library's own code.
#include <moonray/rendering/shading/AttributeKey.h>
#include <moonray/rendering/shading/State.h>
#include <moonray/rendering/shading/Xform.h>
#include <moonray/rendering/shading/ispc/Xform_ispc_stubs.h>

#include "shading_system.h"

#include <OSL/genclosure.h>
#include <OpenImageIO/texture.h>

#include <memory>
#include <mutex>
#include <set>
#include <string>
#include <vector>

namespace nsi_moonray {
namespace {

// The `CLOSURE_*_PARAM` macros name `TypeDesc`, `TypeVector` and the
// rest unqualified; OSL's headers pull them in from OIIO under its own
// namespace, so this is what makes them resolve.
using namespace OSL;

/// What OSL asks the renderer for.
///
/// Deliberately thin. `RendererServices` declares no pure virtuals --
/// everything has a default -- so the surface a renderer must cover is
/// "whatever the shaders actually call", not a fixed list. Transforms
/// are the ones OSL cannot do without, and it asks for them by name
/// through the same interface whether or not a shader mentions one.
///
/// Texturing is OSL's own OIIO `TextureSystem`, not MoonRay's.
/// MoonRay's texture path exists to serve shaders written as ISPC
/// DSOs, which is exactly what OSL replaces
/// (`specs/003-osl/research.md`).
class Services final : public OSL::RendererServices {
public:
    explicit Services(OIIO::TextureSystem* texture)
        : OSL::RendererServices(texture)
    {
    }

    /// A `getattribute()` in a shader, answered from the shading
    /// point's primitive attributes.
    ///
    /// The unscoped form only. OSL's scoped `getattribute("scope",
    /// "name", val)` names a renderer concept -- an object's userdata,
    /// a global setting -- and ɴsɪ has no vocabulary for one, so
    /// answering it would be inventing a mapping. Unanswered means the
    /// shader keeps its own default, which is what `getattribute`
    /// returning 0 tells it.
    bool get_attribute(OSL::ShaderGlobals* globals, bool /*derivatives*/,
                       OSL::ustringhash object, OSL::TypeDesc type,
                       OSL::ustringhash name, void* value) override
    {
        if (!object.empty() || globals == nullptr) {
            return false;
        }
        const auto* point =
            static_cast<const ShadingPoint*>(globals->renderstate);
        if (point == nullptr || point->state == nullptr
            || point->attributes == nullptr) {
            return false;
        }
        return point->attributes->read(*point->state, name, type, value);
    }

    /// A named space, as the matrix that takes it to OSL's *common*
    /// space -- which is MoonRay's render space.
    ///
    /// Built from basis vectors rather than read out of MoonRay's
    /// matrices, and deliberately: `Xform`'s render-to-object entry
    /// resolves through a function pointer per shading point, because
    /// an instanced prototype's object transform is not the material's.
    /// Reading the struct would be right for a plain mesh and silently
    /// wrong for a crowd. Four calls through the documented interface
    /// are exact for every affine transform, which all of these are.
    bool get_matrix(OSL::ShaderGlobals* globals, OSL::Matrix44& result,
                    OSL::ustringhash from, float) override
    {
        int space = 0;
        if (!space_of(from, space)) {
            return false;
        }
        return basis(globals, space, result);
    }

    bool get_inverse_matrix(OSL::ShaderGlobals* globals,
                            OSL::Matrix44& result, OSL::ustringhash to,
                            float) override
    {
        OSL::Matrix44 forward;
        if (!get_matrix(globals, forward, to, 0.0f)) {
            return false;
        }
        result = forward.inverse();
        return true;
    }

    /// The `TransformationPtr` form, which OSL uses for
    /// `sg->object2common` and `sg->shader2common`.
    ///
    /// Those are set to the `ShadingPoint`, so both resolve through
    /// the same path as the named spaces -- object space either way,
    /// since an ɴsɪ shader has no transform of its own to make
    /// "shader space" mean anything else.
    bool get_matrix(OSL::ShaderGlobals* globals, OSL::Matrix44& result,
                    OSL::TransformationPtr, float) override
    {
        return basis(globals, ispc::SHADING_SPACE_OBJECT, result);
    }

private:
    /// OSL's space names, as MoonRay's enum.
    ///
    /// `common` is render space and needs no transform. A name neither
    /// side knows returns false, which is how OSL reports an unknown
    /// space to the shader rather than handing it an identity.
    static bool space_of(OSL::ustringhash name, int& space)
    {
        static const OSL::ustring object("object");
        static const OSL::ustring world("world");
        static const OSL::ustring camera("camera");
        static const OSL::ustring screen("screen");
        static const OSL::ustring shader("shader");
        static const OSL::ustring common("common");

        if (name == OSL::ustringhash(object)
            || name == OSL::ustringhash(shader)) {
            space = ispc::SHADING_SPACE_OBJECT;
        } else if (name == OSL::ustringhash(world)) {
            space = ispc::SHADING_SPACE_WORLD;
        } else if (name == OSL::ustringhash(camera)) {
            space = ispc::SHADING_SPACE_CAMERA;
        } else if (name == OSL::ustringhash(screen)) {
            space = ispc::SHADING_SPACE_SCREEN;
        } else if (name == OSL::ustringhash(common)) {
            space = ispc::SHADING_SPACE_RENDER;
        } else {
            return false;
        }
        return true;
    }

    /// One space's matrix, from where its origin and axes land.
    ///
    /// Imath's `Matrix44` multiplies a *row* vector on the left, so the
    /// first three rows are the mapped axes and the fourth is the
    /// mapped origin.
    static bool basis(OSL::ShaderGlobals* globals, int space,
                      OSL::Matrix44& result)
    {
        const auto* point =
            static_cast<const ShadingPoint*>(globals->renderstate);
        if (point == nullptr || point->xform == nullptr
            || point->state == nullptr) {
            return false;
        }

        const int render = ispc::SHADING_SPACE_RENDER;
        const auto origin = point->xform->transformPoint(
            space, render, *point->state,
            scene_rdl2::math::Vec3f(0.0f, 0.0f, 0.0f));

        scene_rdl2::math::Vec3f axes[3];
        for (int i = 0; i < 3; ++i) {
            scene_rdl2::math::Vec3f unit(0.0f, 0.0f, 0.0f);
            unit[i] = 1.0f;
            axes[i] = point->xform->transformVector(space, render,
                                                    *point->state, unit);
        }

        result = OSL::Matrix44(axes[0].x, axes[0].y, axes[0].z, 0.0f,
                               axes[1].x, axes[1].y, axes[1].z, 0.0f,
                               axes[2].x, axes[2].y, axes[2].z, 0.0f,
                               origin.x, origin.y, origin.z, 1.0f);
        return true;
    }

public:
};

/// Everything the shared system owns, so its lifetime is one object's.
struct System {
    // **OpenImageIO 3 returns a `shared_ptr` from
    // `TextureSystem::create`,** where 2.x returned a raw pointer and
    // asked you to call `destroy`. Held by value so the lifetime is
    // the system's; `Services` and `ShadingSystem` want the raw
    // pointer and do not own it.
    std::shared_ptr<OIIO::TextureSystem> texture_system;
    OIIO::TextureSystem* texture;
    Services services;
    OSL::ShadingSystem shading;
    std::mutex guard;
    std::set<std::string> search_paths;

    System()
        : texture_system(OIIO::TextureSystem::create())
        , texture(texture_system.get())
        , services(texture)
        , shading(&services, texture)
    {
        register_closures();
        // Shaders are instanced per material and their parameters come
        // from the group spec, never from geometry, so OSL may bake
        // them in.
        shading.attribute("lockgeom", 1);
    }

    void register_closures();
};

void
System::register_closures()
{
    constexpr int max_params = 32;
    struct Builtin {
        const char* name;
        int id;
        OSL::ClosureParam params[max_params];
    };

    // `"label"` is registered here rather than being part of OSL: the
    // renderer decides which closures carry one. Every closure that
    // becomes a MoonRay lobe carries it, because a lobe is what a
    // material AOV and an LPE can name.
    Builtin builtins[] = {
        { "emission", CLOSURE_EMISSION,
          { CLOSURE_FINISH_PARAM(EmptyParams) } },

        { "background", CLOSURE_BACKGROUND,
          { CLOSURE_FINISH_PARAM(EmptyParams) } },

        { "diffuse", CLOSURE_DIFFUSE,
          { CLOSURE_VECTOR_PARAM(DiffuseParams, N),
            CLOSURE_STRING_KEYPARAM(DiffuseParams, label, "label"),
            CLOSURE_FINISH_PARAM(DiffuseParams) } },

        { "oren_nayar", CLOSURE_OREN_NAYAR,
          { CLOSURE_VECTOR_PARAM(OrenNayarParams, N),
            CLOSURE_FLOAT_PARAM(OrenNayarParams, sigma),
            CLOSURE_STRING_KEYPARAM(OrenNayarParams, label, "label"),
            CLOSURE_FINISH_PARAM(OrenNayarParams) } },

        { "translucent", CLOSURE_TRANSLUCENT,
          { CLOSURE_VECTOR_PARAM(DiffuseParams, N),
            CLOSURE_STRING_KEYPARAM(DiffuseParams, label, "label"),
            CLOSURE_FINISH_PARAM(DiffuseParams) } },

        { "reflection", CLOSURE_REFLECTION,
          { CLOSURE_VECTOR_PARAM(ReflectionParams, N),
            CLOSURE_FLOAT_PARAM(ReflectionParams, eta),
            CLOSURE_STRING_KEYPARAM(ReflectionParams, label, "label"),
            CLOSURE_FINISH_PARAM(ReflectionParams) } },

        { "refraction", CLOSURE_REFRACTION,
          { CLOSURE_VECTOR_PARAM(RefractionParams, N),
            CLOSURE_FLOAT_PARAM(RefractionParams, eta),
            CLOSURE_STRING_KEYPARAM(RefractionParams, label, "label"),
            CLOSURE_FINISH_PARAM(RefractionParams) } },

        { "transparent", CLOSURE_TRANSPARENT,
          { CLOSURE_FINISH_PARAM(EmptyParams) } },

        { "microfacet", CLOSURE_MICROFACET,
          { CLOSURE_STRING_PARAM(MicrofacetParams, dist),
            CLOSURE_VECTOR_PARAM(MicrofacetParams, N),
            CLOSURE_VECTOR_PARAM(MicrofacetParams, U),
            CLOSURE_FLOAT_PARAM(MicrofacetParams, xalpha),
            CLOSURE_FLOAT_PARAM(MicrofacetParams, yalpha),
            CLOSURE_FLOAT_PARAM(MicrofacetParams, eta),
            CLOSURE_INT_PARAM(MicrofacetParams, refract),
            // 3Delight's conductor spelling. Registered as keywords
            // because that is how its shaders pass them, and because a
            // shader that does not is unaffected -- OSL zeroes the
            // block, and zero is "not a conductor".
            CLOSURE_COLOR_KEYPARAM(MicrofacetParams, realeta, "realeta"),
            CLOSURE_COLOR_KEYPARAM(MicrofacetParams, complexeta,
                                   "complexeta"),
            CLOSURE_STRING_KEYPARAM(MicrofacetParams, label, "label"),
            CLOSURE_FINISH_PARAM(MicrofacetParams) } },

        // Four formals. See `SubsurfaceParams`: a fifth cost a
        // segfault inside OSL's own code generator.
        { "subsurface", CLOSURE_SUBSURFACE,
          { CLOSURE_FLOAT_PARAM(SubsurfaceParams, eta),
            CLOSURE_FLOAT_PARAM(SubsurfaceParams, g),
            CLOSURE_COLOR_PARAM(SubsurfaceParams, mfp),
            CLOSURE_COLOR_PARAM(SubsurfaceParams, albedo),
            CLOSURE_VECTOR_KEYPARAM(SubsurfaceParams, N, "N"),
            CLOSURE_STRING_KEYPARAM(SubsurfaceParams, label, "label"),
            CLOSURE_FINISH_PARAM(SubsurfaceParams) } },

        // MaterialX. Registered with the same layouts OSL's own
        // `testrender` uses, because the struct and the registration
        // are one contract and getting an offset wrong reads garbage
        // rather than failing.
        { "oren_nayar_diffuse_bsdf", CLOSURE_MX_OREN_NAYAR,
          { CLOSURE_VECTOR_PARAM(MxDiffuseParams, N),
            CLOSURE_COLOR_PARAM(MxDiffuseParams, albedo),
            CLOSURE_FLOAT_PARAM(MxDiffuseParams, roughness),
            CLOSURE_STRING_KEYPARAM(MxDiffuseParams, label, "label"),
            CLOSURE_FINISH_PARAM(MxDiffuseParams) } },

        { "burley_diffuse_bsdf", CLOSURE_MX_BURLEY,
          { CLOSURE_VECTOR_PARAM(MxDiffuseParams, N),
            CLOSURE_COLOR_PARAM(MxDiffuseParams, albedo),
            CLOSURE_FLOAT_PARAM(MxDiffuseParams, roughness),
            CLOSURE_STRING_KEYPARAM(MxDiffuseParams, label, "label"),
            CLOSURE_FINISH_PARAM(MxDiffuseParams) } },

        { "dielectric_bsdf", CLOSURE_MX_DIELECTRIC,
          { CLOSURE_VECTOR_PARAM(MxDielectricParams, N),
            CLOSURE_VECTOR_PARAM(MxDielectricParams, U),
            CLOSURE_COLOR_PARAM(MxDielectricParams, reflection_tint),
            CLOSURE_COLOR_PARAM(MxDielectricParams, transmission_tint),
            CLOSURE_FLOAT_PARAM(MxDielectricParams, roughness_x),
            CLOSURE_FLOAT_PARAM(MxDielectricParams, roughness_y),
            CLOSURE_FLOAT_PARAM(MxDielectricParams, ior),
            CLOSURE_STRING_PARAM(MxDielectricParams, distribution),
            CLOSURE_FLOAT_KEYPARAM(MxDielectricParams, thinfilm_thickness,
                                   "thinfilm_thickness"),
            CLOSURE_FLOAT_KEYPARAM(MxDielectricParams, thinfilm_ior,
                                   "thinfilm_ior"),
            CLOSURE_STRING_KEYPARAM(MxDielectricParams, label, "label"),
            CLOSURE_FINISH_PARAM(MxDielectricParams) } },

        { "conductor_bsdf", CLOSURE_MX_CONDUCTOR,
          { CLOSURE_VECTOR_PARAM(MxConductorParams, N),
            CLOSURE_VECTOR_PARAM(MxConductorParams, U),
            CLOSURE_FLOAT_PARAM(MxConductorParams, roughness_x),
            CLOSURE_FLOAT_PARAM(MxConductorParams, roughness_y),
            CLOSURE_COLOR_PARAM(MxConductorParams, ior),
            CLOSURE_COLOR_PARAM(MxConductorParams, extinction),
            CLOSURE_STRING_PARAM(MxConductorParams, distribution),
            CLOSURE_FLOAT_KEYPARAM(MxConductorParams, thinfilm_thickness,
                                   "thinfilm_thickness"),
            CLOSURE_FLOAT_KEYPARAM(MxConductorParams, thinfilm_ior,
                                   "thinfilm_ior"),
            CLOSURE_STRING_KEYPARAM(MxConductorParams, label, "label"),
            CLOSURE_FINISH_PARAM(MxConductorParams) } },

        { "generalized_schlick_bsdf", CLOSURE_MX_GENERALIZED_SCHLICK,
          { CLOSURE_VECTOR_PARAM(MxGeneralizedSchlickParams, N),
            CLOSURE_VECTOR_PARAM(MxGeneralizedSchlickParams, U),
            CLOSURE_COLOR_PARAM(MxGeneralizedSchlickParams, reflection_tint),
            CLOSURE_COLOR_PARAM(MxGeneralizedSchlickParams,
                                transmission_tint),
            CLOSURE_FLOAT_PARAM(MxGeneralizedSchlickParams, roughness_x),
            CLOSURE_FLOAT_PARAM(MxGeneralizedSchlickParams, roughness_y),
            CLOSURE_COLOR_PARAM(MxGeneralizedSchlickParams, f0),
            CLOSURE_COLOR_PARAM(MxGeneralizedSchlickParams, f90),
            CLOSURE_FLOAT_PARAM(MxGeneralizedSchlickParams, exponent),
            CLOSURE_STRING_PARAM(MxGeneralizedSchlickParams, distribution),
            CLOSURE_FLOAT_KEYPARAM(MxGeneralizedSchlickParams,
                                   thinfilm_thickness, "thinfilm_thickness"),
            CLOSURE_FLOAT_KEYPARAM(MxGeneralizedSchlickParams, thinfilm_ior,
                                   "thinfilm_ior"),
            CLOSURE_STRING_KEYPARAM(MxGeneralizedSchlickParams, label,
                                    "label"),
            CLOSURE_FINISH_PARAM(MxGeneralizedSchlickParams) } },

        { "translucent_bsdf", CLOSURE_MX_TRANSLUCENT,
          { CLOSURE_VECTOR_PARAM(MxTranslucentParams, N),
            CLOSURE_COLOR_PARAM(MxTranslucentParams, albedo),
            CLOSURE_STRING_KEYPARAM(MxTranslucentParams, label, "label"),
            CLOSURE_FINISH_PARAM(MxTranslucentParams) } },

        { "transparent_bsdf", CLOSURE_MX_TRANSPARENT,
          { CLOSURE_FINISH_PARAM(EmptyParams) } },

        { "subsurface_bssrdf", CLOSURE_MX_SUBSURFACE,
          { CLOSURE_VECTOR_PARAM(MxSubsurfaceParams, N),
            CLOSURE_COLOR_PARAM(MxSubsurfaceParams, albedo),
            CLOSURE_FLOAT_PARAM(MxSubsurfaceParams, transmission_depth),
            CLOSURE_COLOR_PARAM(MxSubsurfaceParams, transmission_color),
            CLOSURE_FLOAT_PARAM(MxSubsurfaceParams, anisotropy),
            CLOSURE_STRING_KEYPARAM(MxSubsurfaceParams, label, "label"),
            CLOSURE_FINISH_PARAM(MxSubsurfaceParams) } },

        { "sheen_bsdf", CLOSURE_MX_SHEEN,
          { CLOSURE_VECTOR_PARAM(MxSheenParams, N),
            CLOSURE_COLOR_PARAM(MxSheenParams, albedo),
            CLOSURE_FLOAT_PARAM(MxSheenParams, roughness),
            CLOSURE_STRING_KEYPARAM(MxSheenParams, label, "label"),
            CLOSURE_FINISH_PARAM(MxSheenParams) } },

        { "uniform_edf", CLOSURE_MX_UNIFORM_EDF,
          { CLOSURE_COLOR_PARAM(MxUniformEdfParams, emittance),
            CLOSURE_STRING_KEYPARAM(MxUniformEdfParams, label, "label"),
            CLOSURE_FINISH_PARAM(MxUniformEdfParams) } },

        { "layer", CLOSURE_MX_LAYER,
          { CLOSURE_CLOSURE_PARAM(MxLayerParams, top),
            CLOSURE_CLOSURE_PARAM(MxLayerParams, base),
            CLOSURE_FINISH_PARAM(MxLayerParams) } },

        // 3Delight's extensions. Declared in its `3delightosl.h`
        // rather than in OSL, and unavoidable: every shader 3Delight
        // ships builds its `Ci` out of `layer_closures` and wraps each
        // part in an `outputvariable`, so a renderer that does not know
        // them renders those shaders black.
        { "layer_closures", CLOSURE_DL_LAYER,
          { CLOSURE_CLOSURE_PARAM(DlLayerParams, top),
            CLOSURE_CLOSURE_PARAM(DlLayerParams, bottom),
            CLOSURE_COLOR_PARAM(DlLayerParams, top_mask),
            CLOSURE_FINISH_PARAM(DlLayerParams) } },

        { "outputvariable", CLOSURE_DL_OUTPUT_VARIABLE,
          { CLOSURE_STRING_PARAM(DlOutputVariableParams, name),
            CLOSURE_CLOSURE_PARAM(DlOutputVariableParams, value),
            CLOSURE_FINISH_PARAM(DlOutputVariableParams) } },

        { "outputconstant", CLOSURE_DL_OUTPUT_CONSTANT,
          { CLOSURE_STRING_PARAM(DlOutputConstantParams, name),
            CLOSURE_FINISH_PARAM(DlOutputConstantParams) } },

        { "occlusion", CLOSURE_DL_OCCLUSION,
          { CLOSURE_VECTOR_PARAM(DlOcclusionParams, N),
            CLOSURE_FINISH_PARAM(DlOcclusionParams) } },
    };

    for (const Builtin& builtin : builtins) {
        shading.register_closure(builtin.name, builtin.id, builtin.params,
                                 nullptr, nullptr);
    }
}

System&
system()
{
    // Leaked on purpose. OSL's JIT state outlives any one render, and
    // destroying a `ShadingSystem` while a `ShaderGroup` is still
    // alive is undefined -- which, in a `dlopen`ed renderer, means at
    // static destruction time in an order nobody controls.
    static System* the_system = new System();
    return *the_system;
}

} // namespace

OSL::ShadingSystem&
shading_system()
{
    return system().shading;
}

namespace {

scene_rdl2::math::Color
to_colour(const OSL::Color3& colour)
{
    return scene_rdl2::math::Color(colour.x, colour.y, colour.z);
}

/// One walk of a closure tree, summing whatever `keep` picks out.
///
/// The two questions that need a tree walk without a `BsdfBuilder` --
/// what does this emit, and what does it let through -- differ only in
/// which components count, so they share the descent. Weights fold
/// down it the same way the lobe walk folds them.
template <typename Keep>
scene_rdl2::math::Color
sum(const OSL::ClosureColor* closure, const scene_rdl2::math::Color& weight,
    const Keep& keep)
{
    if (closure == nullptr) {
        return scene_rdl2::math::sBlack;
    }

    switch (closure->id) {
    case OSL::ClosureColor::MUL: {
        const auto* mul = closure->as_mul();
        return sum(mul->closure, weight * to_colour(mul->weight), keep);
    }
    case OSL::ClosureColor::ADD: {
        const auto* add = closure->as_add();
        return sum(add->closureA, weight, keep)
             + sum(add->closureB, weight, keep);
    }
    default: {
        const auto* component = closure->as_comp();
        // The closures that carry other closures rather than
        // scattering themselves. Descending through them is what makes
        // a 3Delight shader's emission reachable at all: every part of
        // its `Ci` is wrapped in an `outputvariable` and layered with
        // `layer_closures`.
        switch (component->id) {
        case CLOSURE_MX_LAYER: {
            const auto* params = component->as<MxLayerParams>();
            return sum(params->top, weight, keep)
                 + sum(params->base, weight, keep);
        }
        case CLOSURE_DL_LAYER: {
            const auto* params = component->as<DlLayerParams>();
            return sum(params->top, weight * to_colour(params->top_mask),
                       keep)
                 + sum(params->bottom, weight, keep);
        }
        case CLOSURE_DL_OUTPUT_VARIABLE: {
            const auto* params = component->as<DlOutputVariableParams>();
            return sum(params->value, weight, keep);
        }
        default:
            return keep(component, weight * to_colour(component->w));
        }
    }
    }
}

} // namespace

const OSL::ClosureColor*
execute(const OSL::ShaderGroupRef& group,
        const moonray::shading::Xform* xform, const Attributes& attributes,
        const moonray::shading::State& state)
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

scene_rdl2::math::Color
emission_of(const OSL::ClosureColor* closure,
            const scene_rdl2::math::Color& weight)
{
    return sum(closure, weight,
               [](const OSL::ClosureComponent* component,
                  const scene_rdl2::math::Color& weight) {
                   switch (component->id) {
                   case CLOSURE_EMISSION:
                       // `emission()` has no parameters: the weight it
                       // was multiplied by *is* the radiance.
                       return weight;
                   case CLOSURE_MX_UNIFORM_EDF:
                       return weight
                              * to_colour(
                                  component->as<MxUniformEdfParams>()
                                      ->emittance);
                   default:
                       return scene_rdl2::math::sBlack;
                   }
               });
}

scene_rdl2::math::Color
transparency(const OSL::ClosureColor* closure,
             const scene_rdl2::math::Color& weight)
{
    return sum(closure, weight,
               [](const OSL::ClosureComponent* component,
                  const scene_rdl2::math::Color& weight) {
                   return component->id == CLOSURE_TRANSPARENT
                                  || component->id == CLOSURE_MX_TRANSPARENT
                              ? weight
                              : scene_rdl2::math::sBlack;
               });
}

namespace {

/// One OSL type, as the MoonRay primitive-attribute key for a name.
///
/// -1 for a type MoonRay has no primitive attribute for. The list is
/// MoonRay's, not OSL's: `AttributeKey` is templated on the storage
/// type, and there are only so many.
int
key_of(const std::string& name, OSL::TypeDesc type)
{
    using namespace moonray::shading;

    if (type == ::OIIO::TypeFloat) {
        return TypedAttributeKey<float>(name);
    }
    if (type == ::OIIO::TypeInt) {
        return TypedAttributeKey<int>(name);
    }
    if (type == ::OIIO::TypeColor) {
        return TypedAttributeKey<scene_rdl2::math::Color>(name);
    }
    if (type == ::OIIO::TypePoint || type == ::OIIO::TypeVector
        || type == ::OIIO::TypeNormal) {
        return TypedAttributeKey<scene_rdl2::math::Vec3f>(name);
    }
    // OSL has no two-float type of its own; `float[2]` is how a shader
    // spells a UV set other than `st`.
    if (type.basetype == OSL::TypeDesc::FLOAT && type.aggregate == 1
        && type.arraylen == 2) {
        return TypedAttributeKey<scene_rdl2::math::Vec2f>(name);
    }
    if (type == ::OIIO::TypeString) {
        return TypedAttributeKey<std::string>(name);
    }
    return -1;
}

} // namespace

Attributes
Attributes::of(const OSL::ShaderGroupRef& group)
{
    Attributes attributes;
    if (!group) {
        return attributes;
    }

    OSL::ShadingSystem& shading = shading_system();

    int count = 0;
    OSL::ustring* names = nullptr;
    OSL::ustring* scopes = nullptr;
    OSL::TypeDesc* types = nullptr;
    if (!shading.getattribute(group.get(), "num_attributes_needed",
                              OSL::TypeDesc::INT, &count)
        || !shading.getattribute(group.get(), "attributes_needed",
                                 OSL::TypeDesc::PTR, &names)
        || !shading.getattribute(group.get(), "attribute_scopes",
                                 OSL::TypeDesc::PTR, &scopes)
        || !shading.getattribute(group.get(), "attribute_types",
                                 OSL::TypeDesc::PTR, &types)
        || names == nullptr || scopes == nullptr || types == nullptr) {
        return attributes;
    }

    for (int i = 0; i < count; ++i) {
        // Scoped queries name a renderer concept ɴsɪ has no vocabulary
        // for; see `Services::get_attribute`.
        if (!scopes[i].empty()) {
            continue;
        }
        const int key = key_of(names[i].string(), types[i]);
        if (key < 0) {
            continue;
        }
        attributes.mEntries.push_back({ names[i], types[i], key });
        attributes.mKeys.push_back(key);
    }

    return attributes;
}

bool
Attributes::read(const moonray::shading::State& state, OSL::ustringhash name,
                 OSL::TypeDesc type, void* value) const
{
    using namespace moonray::shading;

    for (const Entry& entry : mEntries) {
        if (OSL::ustringhash(entry.name) != name || entry.type != type) {
            continue;
        }
        const AttributeKey key(entry.key);
        if (!state.isProvided(key)) {
            return false;
        }

        if (type == ::OIIO::TypeFloat) {
            *static_cast<float*>(value) =
                state.getAttribute(TypedAttributeKey<float>(key));
            return true;
        }
        if (type == ::OIIO::TypeInt) {
            *static_cast<int*>(value) =
                state.getAttribute(TypedAttributeKey<int>(key));
            return true;
        }
        if (type == ::OIIO::TypeColor) {
            const scene_rdl2::math::Color& colour = state.getAttribute(
                TypedAttributeKey<scene_rdl2::math::Color>(key));
            auto* out = static_cast<float*>(value);
            out[0] = colour.r;
            out[1] = colour.g;
            out[2] = colour.b;
            return true;
        }
        if (type == ::OIIO::TypePoint
            || type == ::OIIO::TypeVector
            || type == ::OIIO::TypeNormal) {
            const scene_rdl2::math::Vec3f& vector = state.getAttribute(
                TypedAttributeKey<scene_rdl2::math::Vec3f>(key));
            auto* out = static_cast<float*>(value);
            out[0] = vector.x;
            out[1] = vector.y;
            out[2] = vector.z;
            return true;
        }
        if (type.basetype == OSL::TypeDesc::FLOAT && type.aggregate == 1
            && type.arraylen == 2) {
            const scene_rdl2::math::Vec2f& uv = state.getAttribute(
                TypedAttributeKey<scene_rdl2::math::Vec2f>(key));
            auto* out = static_cast<float*>(value);
            out[0] = uv.x;
            out[1] = uv.y;
            return true;
        }
        if (type == ::OIIO::TypeString) {
            *static_cast<OSL::ustring*>(value) = OSL::ustring(
                state.getAttribute(TypedAttributeKey<std::string>(key)));
            return true;
        }
        return false;
    }

    return false;
}

void
add_search_path(const std::string& path)
{
    if (path.empty()) {
        return;
    }

    System& live = system();
    const std::lock_guard<std::mutex> held(live.guard);

    if (!live.search_paths.insert(path).second) {
        return;
    }

    std::string joined;
    for (const std::string& one : live.search_paths) {
        if (!joined.empty()) {
            joined += ':';
        }
        joined += one;
    }
    live.shading.attribute("searchpath:shader", joined);
}

} // namespace nsi_moonray
