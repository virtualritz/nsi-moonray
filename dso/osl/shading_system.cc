// **rdl2's headers first, and it matters.**
// `scene_rdl2/render/util/AtomicFloat.h` *specialises*
// `std::atomic<float>`, and OIIO -- which OSL's headers pull in --
// instantiates it. Whichever comes second loses, with
// "specialization of 'std::atomic<float>' after instantiation" and a
// backtrace that points at neither library's own code.
#include <moonray/rendering/shading/State.h>
#include <moonray/rendering/shading/Xform.h>
#include <moonray/rendering/shading/ispc/Xform_ispc_stubs.h>

#include "shading_system.h"

#include <OSL/genclosure.h>
#include <OpenImageIO/texture.h>

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
    OIIO::TextureSystem* texture;
    Services services;
    OSL::ShadingSystem shading;
    std::mutex guard;
    std::set<std::string> search_paths;

    System()
        : texture(OIIO::TextureSystem::create())
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
            CLOSURE_STRING_KEYPARAM(MicrofacetParams, label, "label"),
            CLOSURE_FINISH_PARAM(MicrofacetParams) } },
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
