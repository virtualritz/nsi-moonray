/// See `attributes.cc`: a stand-in for the class MoonRay's `MeshLight`
/// requires and `moonray` does not ship.
///
/// It shades nothing. The mesh-light layer is not rendered as geometry
/// -- it exists to tessellate the light's mesh and to gather the map
/// shader's primitive-attribute requests -- so `shade` is reached only
/// if MoonRay's own assumptions change, and adding no lobes is then the
/// honest answer rather than a guess at what a DreamWorks material
/// would have done.

#include "attributes.cc"

#include <moonray/rendering/shading/MaterialApi.h>

#include <string>

using namespace moonray::shading;

RDL2_DSO_CLASS_BEGIN(DwaBaseMaterial, scene_rdl2::rdl2::Material)

public:
    DwaBaseMaterial(const scene_rdl2::rdl2::SceneClass& sceneClass,
                    const std::string& name);

    static void shade(const scene_rdl2::rdl2::Material* self,
                      moonray::shading::TLState* tls,
                      const State& state,
                      BsdfBuilder& bsdfBuilder);

RDL2_DSO_CLASS_END(DwaBaseMaterial)

DwaBaseMaterial::DwaBaseMaterial(
    const scene_rdl2::rdl2::SceneClass& sceneClass, const std::string& name)
    : Parent(sceneClass, name)
{
    mShadeFunc = DwaBaseMaterial::shade;
    mShadeFuncv = nullptr;
}

void
DwaBaseMaterial::shade(const scene_rdl2::rdl2::Material* /*self*/,
                       moonray::shading::TLState* /*tls*/,
                       const State& /*state*/,
                       BsdfBuilder& /*bsdfBuilder*/)
{
}
