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

    // Identity for now: an ɴsɪ scene's transforms are resolved
    // upstream and baked into geometry before MoonRay sees them, so a
    // shader asking for `object` or `world` space gets render space
    // and the two coincide. A shader that depends on the difference
    // will be wrong, quietly, which is why this is a known gap rather
    // than a finished implementation.
    bool get_matrix(OSL::ShaderGlobals*, OSL::Matrix44& result,
                    OSL::TransformationPtr, float) override
    {
        result.makeIdentity();
        return true;
    }

    bool get_matrix(OSL::ShaderGlobals*, OSL::Matrix44& result,
                    OSL::ustringhash, float) override
    {
        result.makeIdentity();
        return true;
    }

    bool get_inverse_matrix(OSL::ShaderGlobals*, OSL::Matrix44& result,
                            OSL::ustringhash, float) override
    {
        result.makeIdentity();
        return true;
    }
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
