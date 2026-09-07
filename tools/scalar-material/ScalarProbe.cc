/// A `Material` with a scalar `shade` and **no vectorised one**.
///
/// Every material MoonRay ships is built with `moonray_ispc_dso`, so
/// the scalar-only path is never exercised by its own shaders. OSL has
/// no ISPC path to give -- its `ShadingSystem` shades one point at a
/// time -- so what this backend would ship is exactly this shape, and
/// what it does under each execution mode has to be measured before
/// anything is built on it.
///
/// One flat white Lambertian lobe. Nothing else, so that a black
/// render can only mean the material was not run.

#include "attributes.cc"

#include <moonray/rendering/shading/MaterialApi.h>

using namespace moonray::shading;

RDL2_DSO_CLASS_BEGIN(ScalarProbe, scene_rdl2::rdl2::Material)

public:
    ScalarProbe(const scene_rdl2::rdl2::SceneClass& sceneClass,
                const std::string& name);

    static void shade(const scene_rdl2::rdl2::Material* self,
                      moonray::shading::TLState* tls,
                      const State& state,
                      BsdfBuilder& bsdfBuilder);

RDL2_DSO_CLASS_END(ScalarProbe)

ScalarProbe::ScalarProbe(const scene_rdl2::rdl2::SceneClass& sceneClass,
                         const std::string& name)
    : Parent(sceneClass, name)
{
    mShadeFunc = ScalarProbe::shade;
    // Deliberately left null. `Material::shadev` null-checks it and
    // does nothing, which is the whole question.
    mShadeFuncv = nullptr;
}

void
ScalarProbe::shade(const scene_rdl2::rdl2::Material* self,
                   moonray::shading::TLState* /*tls*/,
                   const State& state,
                   BsdfBuilder& bsdfBuilder)
{
    const ScalarProbe* me = static_cast<const ScalarProbe*>(self);
    const scene_rdl2::math::Color albedo = me->get(attrColor);

    const LambertianBRDF lambert(state.getN(), albedo);
    bsdfBuilder.addLambertianBRDF(lambert,
                                  1.0f,
                                  ispc::BSDFBUILDER_PHYSICAL,
                                  0);
}
