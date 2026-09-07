/// An OSL displacement shader, as a MoonRay `Displacement`.
///
/// ɴsɪ binds one through the `attributes` node's `displacementshader`
/// connection, the way it binds a surface shader through
/// `surfaceshader`. The group that arrives here is built by the same
/// code that builds a material's -- the difference is OSL's *usage*,
/// `"displacement"` rather than `"surface"`, which is what lets a
/// shader assign to `P`.
///
/// # What a displacement shader returns
///
/// OSL has no displacement closure. A displacement shader moves `P`
/// (and may re-shade `N`), so the displacement MoonRay wants is the
/// difference between the `P` the shader was handed and the `P` it
/// left behind. `N` is not carried back: MoonRay recomputes shading
/// normals from the displaced surface itself, and handing it a normal
/// the geometry does not have is worse than letting it.
///
/// # Scalar only
///
/// `mDisplaceFuncv` is null for the reason `Osl.cc` explains at
/// length: OSL shades one point at a time. Displacement runs at
/// tessellation rather than in the shading loop, so the cost lands
/// once per render rather than per sample -- but the vectorized entry
/// point still has to be refused rather than faked.

#define NSI_MOONRAY_OSL_ROOT rdl2::Displacement
#include "attributes.cc"
#include "shading_system.h"

#include <moonray/rendering/shading/State.h>
#include <moonray/rendering/shading/Xform.h>

#include <scene_rdl2/scene/rdl2/rdl2.h>

#include <cstdlib>
#include <memory>
#include <string>

using namespace moonray::shading;
using namespace nsi_moonray;

RDL2_DSO_CLASS_BEGIN(OslDisplacement, scene_rdl2::rdl2::Displacement)

public:
    OslDisplacement(const scene_rdl2::rdl2::SceneClass& sceneClass,
                    const std::string& name);

    void update() override;

    static void displace(const scene_rdl2::rdl2::Displacement* self,
                         moonray::shading::TLState* tls,
                         const State& state,
                         scene_rdl2::math::Vec3f* displacement);

private:
    OSL::ShaderGroupRef mGroup;
    /// The scene's spaces, so `transform("object", P)` means what it
    /// says here as much as it does in a surface shader.
    std::unique_ptr<Xform> mXform;
    /// The primitive attributes this group's shaders read. A
    /// displacement reads them as much as a surface does -- more, in
    /// practice, since that is where a height map lives.
    Attributes mAttributes;

RDL2_DSO_CLASS_END(OslDisplacement)

OslDisplacement::OslDisplacement(
    const scene_rdl2::rdl2::SceneClass& sceneClass, const std::string& name)
    : Parent(sceneClass, name)
{
    mDisplaceFunc = OslDisplacement::displace;
    mDisplaceFuncv = nullptr;
}

void
OslDisplacement::update()
{
    mGroup.reset();
    mXform = std::make_unique<Xform>(this);

    const std::string& spec = get(attrGroupSpec);
    if (spec.empty()) {
        error("no OSL group specification; this displacement moves "
              "nothing");
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
                                      "displacement", spec);
    if (!mGroup) {
        error("the OSL group specification did not parse; this "
              "displacement moves nothing");
        return;
    }
    shading.ShaderGroupEnd(*mGroup);

    shading.optimize_group(mGroup.get(), nullptr, true);
    mAttributes = Attributes::of(mGroup);
    mOptionalAttributes = mAttributes.keys();
}

void
OslDisplacement::displace(const scene_rdl2::rdl2::Displacement* self,
                          moonray::shading::TLState* /*tls*/,
                          const State& state,
                          scene_rdl2::math::Vec3f* displacement)
{
    const OslDisplacement* me = static_cast<const OslDisplacement*>(self);
    *displacement = scene_rdl2::math::Vec3f(0.0f, 0.0f, 0.0f);
    if (!me->mGroup) {
        return;
    }

    OSL::ShadingSystem& shading = shading_system();

    thread_local OSL::PerThreadInfo* threadInfo =
        shading.create_thread_info();
    thread_local OSL::ShadingContext* context =
        shading.get_context(threadInfo);

    const scene_rdl2::math::Vec3f& position = state.getP();
    const scene_rdl2::math::Vec3f& normal = state.getN();
    const scene_rdl2::math::Vec3f& geometric = state.getNg();
    const scene_rdl2::math::Vec2f& st = state.getSt();
    const scene_rdl2::math::Vec3f& dPds = state.getdPds();
    const scene_rdl2::math::Vec3f& dPdt = state.getdPdt();

    OSL::ShaderGlobals globals = {};
    globals.P = OSL::Vec3(position.x, position.y, position.z);
    globals.N = OSL::Vec3(normal.x, normal.y, normal.z);
    globals.Ng = OSL::Vec3(geometric.x, geometric.y, geometric.z);
    // There is no ray at tessellation time, so `I` is zero and a
    // displacement shader that reads it gets what it deserves.
    globals.u = st.x;
    globals.v = st.y;
    globals.dPdu = OSL::Vec3(dPds.x, dPds.y, dPds.z);
    globals.dPdv = OSL::Vec3(dPdt.x, dPdt.y, dPdt.z);
    globals.surfacearea = 1.0f;
    globals.flipHandedness = 0;
    globals.raytype = 1;

    const ShadingPoint point { me->mXform.get(), &state, &me->mAttributes };
    globals.renderstate = const_cast<ShadingPoint*>(&point);
    globals.object2common =
        reinterpret_cast<OSL::TransformationPtr>(&point);
    globals.shader2common =
        reinterpret_cast<OSL::TransformationPtr>(&point);

    shading.execute(context, *me->mGroup, globals);

    *displacement = scene_rdl2::math::Vec3f(globals.P.x - position.x,
                                            globals.P.y - position.y,
                                            globals.P.z - position.z);
}
