/// Does OSL run here, and does it answer what the mapping needs?
///
/// Three questions, in order of how much of the design rests on them:
///
/// 1. Does a **group specification string** work -- `ShaderGroupBegin`'s
///    text form, which is the answer to rdl2 having no dynamic
///    attributes (`research.md` O6)? If it does, the flush's job is a
///    text transformation and can be tested without a renderer.
/// 2. Does executing a shader hand back a **closure tree** this can
///    walk into `BsdfBuilder` calls?
/// 3. Do closure **labels** survive, so MoonRay's AOV and LPE labels
///    have something to map from (`research.md` O5)?
///
/// Not a unit test and not shipped: a probe, in the sense
/// `tools/probe` and `tools/scalar-material` are.

#include <OSL/oslexec.h>
#include <OSL/oslclosure.h>
#include <OSL/rendererservices.h>
#include <OSL/genclosure.h>
#include <OpenImageIO/texture.h>

#include <cstdio>
#include <string>
#include <vector>

using namespace OSL;

namespace {

/// Closure ids, in the order this registers them.
enum ClosureId {
    EMISSION_ID,
    DIFFUSE_ID,
    MICROFACET_ID,
};

struct EmptyParams {};
struct DiffuseParams {
    Vec3 N;
    ustring label;
};
struct MicrofacetParams {
    ustring dist;
    Vec3 N;
    Vec3 U;
    float xalpha, yalpha, eta;
    int refract;
    ustring label;
};

/// The least a renderer can supply. Everything `RendererServices`
/// declares has a default, so a probe overrides only what it is asked
/// for -- which is itself worth knowing: the surface a real
/// implementation must cover is "whatever the shaders actually call",
/// not a fixed list of pure virtuals.
class Services final : public RendererServices {
public:
    explicit Services(OIIO::TextureSystem* texture)
        : RendererServices(texture)
    {
    }

    bool get_matrix(ShaderGlobals*, Matrix44& result, TransformationPtr,
                    float) override
    {
        result.makeIdentity();
        return true;
    }

    bool get_matrix(ShaderGlobals*, Matrix44& result, ustringhash,
                    float) override
    {
        result.makeIdentity();
        return true;
    }

    bool get_inverse_matrix(ShaderGlobals*, Matrix44& result, ustringhash,
                            float) override
    {
        result.makeIdentity();
        return true;
    }
};

void
register_closures(ShadingSystem& shading)
{
    constexpr int max_params = 32;
    struct Builtin {
        const char* name;
        int id;
        ClosureParam params[max_params];
    };

    // `label` is not part of OSL: it is a keyword parameter the
    // *renderer* registers, which is why the AOV question is ours to
    // answer rather than the language's. OSL's own `testrender`
    // registers it the same way.
    Builtin builtins[] = {
        { "emission", EMISSION_ID, { CLOSURE_FINISH_PARAM(EmptyParams) } },
        { "diffuse",
          DIFFUSE_ID,
          { CLOSURE_VECTOR_PARAM(DiffuseParams, N),
            CLOSURE_STRING_KEYPARAM(DiffuseParams, label, "label"),
            CLOSURE_FINISH_PARAM(DiffuseParams) } },
        { "microfacet",
          MICROFACET_ID,
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

/// Walk a closure tree the way a `BsdfBuilder` would have to.
///
/// OSL hands back a tree of `add`, `mul` and component nodes and the
/// renderer flattens it; `BsdfBuilder` takes lobes in order with its
/// own layering. This prints the flattening rather than performing
/// it, which is enough to see that the shape is what the mapping
/// assumes.
void
walk(const ClosureColor* closure, const Color3& weight, int depth)
{
    if (!closure) {
        return;
    }

    const std::string indent(depth * 2, ' ');

    switch (closure->id) {
    case ClosureColor::MUL: {
        const ClosureMul* mul = closure->as_mul();
        walk(mul->closure, weight * mul->weight, depth);
        return;
    }
    case ClosureColor::ADD: {
        const ClosureAdd* add = closure->as_add();
        walk(add->closureA, weight, depth);
        walk(add->closureB, weight, depth);
        return;
    }
    default: {
        const ClosureComponent* component = closure->as_comp();
        const Color3 total = weight * Color3(component->w);

        switch (component->id) {
        case EMISSION_ID:
            std::printf("%semission            weight %g %g %g\n",
                        indent.c_str(), total.x, total.y, total.z);
            break;
        case DIFFUSE_ID: {
            const auto* params = component->as<DiffuseParams>();
            std::printf("%sdiffuse             weight %g %g %g  N %g %g %g"
                        "  label %s\n",
                        indent.c_str(), total.x, total.y, total.z, params->N.x,
                        params->N.y, params->N.z,
                        params->label.empty() ? "(none)"
                                              : params->label.c_str());
            break;
        }
        case MICROFACET_ID: {
            const auto* params = component->as<MicrofacetParams>();
            std::printf("%smicrofacet(%s)  weight %g %g %g  alpha %g"
                        "  refract %d  label %s\n",
                        indent.c_str(), params->dist.c_str(), total.x, total.y,
                        total.z, params->xalpha, params->refract,
                        params->label.empty() ? "(none)"
                                              : params->label.c_str());
            break;
        }
        default:
            std::printf("%sclosure id %d, unrecognised\n", indent.c_str(),
                        component->id);
            break;
        }
    }
    }
}

} // namespace

int
main(int argc, char** argv)
{
    const std::string search_path = argc > 1 ? argv[1] : ".";

    OIIO::TextureSystem* texture = OIIO::TextureSystem::create();
    Services services(texture);
    ShadingSystem shading(&services, texture);

    register_closures(shading);
    shading.attribute("searchpath:shader", search_path);
    shading.attribute("lockgeom", 1);

    // **The question O6 rests on.** Parameters, layers and connections
    // as one string, which is what an rdl2 `String` attribute can
    // carry and what a class with statically declared attributes
    // otherwise cannot.
    const char* group_spec = R"(
        param color Cs 0.2 0.7 0.9 ;
        param float power 42 ;
        shader probe layer1 ;
    )";

    ShaderGroupRef group = shading.ShaderGroupBegin("nsi", "surface",
                                                    group_spec);
    if (!group) {
        std::fprintf(stderr, "the group specification did not parse\n");
        return 1;
    }
    shading.ShaderGroupEnd(*group);

    PerThreadInfo* thread_info = shading.create_thread_info();
    ShadingContext* context = shading.get_context(thread_info);

    ShaderGlobals globals = {};
    globals.P = Vec3(0.0f, 0.0f, 0.0f);
    globals.N = Vec3(0.0f, 0.0f, 1.0f);
    globals.Ng = globals.N;
    globals.I = Vec3(0.0f, 0.0f, -1.0f);
    globals.u = 0.5f;
    globals.v = 0.5f;
    globals.surfacearea = 1.0f;
    globals.backfacing = 0;

    shading.execute(context, *group, globals);

    std::printf("closure tree:\n");
    walk(globals.Ci, Color3(1.0f, 1.0f, 1.0f), 1);

    shading.release_context(context);
    shading.destroy_thread_info(thread_info);
    OIIO::TextureSystem::destroy(texture);

    return globals.Ci ? 0 : 1;
}
