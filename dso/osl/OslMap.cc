/// An OSL network's *emission*, as a MoonRay `Map`.
///
/// This is what makes an OSL light a light rather than a bright
/// surface. MoonRay's self-emission is **hit-only**: the integrator
/// adds `bsdf->getSelfEmission()` when a path happens to strike the
/// surface and does nothing else -- no next-event estimation, no
/// shadow rays -- so an emissive shader lights nothing around it.
/// `specs/003-osl/research.md` O2.
///
/// What does light a scene in MoonRay is a `MeshLight`, which
/// importance-samples a mesh's surface. Its radiance is `color` times
/// `intensity`, one value for the whole mesh -- unless it is given a
/// `map_shader`, which it samples per point with a full shading
/// `State`: position, normal, `uv` and the primitive attributes the
/// map asked for.
///
/// So this class runs the same shader network the surface runs, walks
/// the closure tree for emission alone, and hands back the colour. An
/// OSL light whose emission is a 3D noise in colour *and* intensity is
/// then a light that varies over its own surface, sampled properly
/// rather than seen only where a ray lands on it.
///
/// `emission_of` is shared with the `Osl` material rather than
/// reimplemented, so a light and the surface it is cannot disagree
/// about how bright the surface is.

#define NSI_MOONRAY_OSL_ROOT rdl2::Map
#include "attributes.cc"
#include "shading_system.h"

#include <moonray/rendering/shading/MapApi.h>

#include <cstdlib>
#include <memory>
#include <string>

using namespace moonray::shading;
using namespace nsi_moonray;

RDL2_DSO_CLASS_BEGIN(OslMap, scene_rdl2::rdl2::Map)

public:
    OslMap(const scene_rdl2::rdl2::SceneClass& sceneClass,
           const std::string& name);

    void update() override;

    static void sample(const scene_rdl2::rdl2::Map* self,
                       moonray::shading::TLState* tls,
                       const State& state,
                       scene_rdl2::math::Color* result);

private:
    OSL::ShaderGroupRef mGroup;
    std::unique_ptr<Xform> mXform;
    Attributes mAttributes;

RDL2_DSO_CLASS_END(OslMap)

OslMap::OslMap(const scene_rdl2::rdl2::SceneClass& sceneClass,
               const std::string& name)
    : Parent(sceneClass, name)
{
    mSampleFunc = OslMap::sample;
    // Null for the reason `Osl.cc` explains: OSL shades one point at a
    // time and has no ISPC function to offer.
    mSampleFuncv = nullptr;
}

void
OslMap::update()
{
    mGroup.reset();
    mXform = std::make_unique<Xform>(this);

    const std::string& spec = get(attrGroupSpec);
    if (spec.empty()) {
        error("no OSL group specification; this map emits nothing");
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

    // `"surface"`, because this is a surface shader -- the same one the
    // material runs. Only what it *emits* is read here.
    mGroup = shading.ShaderGroupBegin(groupName.empty() ? getName()
                                                        : groupName,
                                      "surface", spec);
    if (!mGroup) {
        error("the OSL group specification did not parse; this map emits "
              "nothing");
        return;
    }
    shading.ShaderGroupEnd(*mGroup);

    shading.optimize_group(mGroup.get(), nullptr, true);
    mAttributes = Attributes::of(mGroup);
    mOptionalAttributes = mAttributes.keys();
}

void
OslMap::sample(const scene_rdl2::rdl2::Map* self,
               moonray::shading::TLState* /*tls*/,
               const State& state,
               scene_rdl2::math::Color* result)
{
    const OslMap* me = static_cast<const OslMap*>(self);
    *result = scene_rdl2::math::sBlack;
    if (!me->mGroup) {
        return;
    }

    *result = emission_of(
        execute(me->mGroup, me->mXform.get(), me->mAttributes, state),
        scene_rdl2::math::sWhite);
}
