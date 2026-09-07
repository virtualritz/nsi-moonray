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

#include <moonray/rendering/shading/AttributeKey.h>
#include <moonray/rendering/shading/MaterialApi.h>

#include <string>

using namespace moonray::shading;

RDL2_DSO_CLASS_BEGIN(DwaBaseMaterial, scene_rdl2::rdl2::Material)

public:
    DwaBaseMaterial(const scene_rdl2::rdl2::SceneClass& sceneClass,
                    const std::string& name);

    void update() override;

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
DwaBaseMaterial::update()
{
    // **The texture coordinates, asked for explicitly.**
    //
    // `MeshLight::sampleMapShader` calls `Mesh::getST` on the light's
    // mesh before handing a shading state to the map, and `getST`
    // reaches into the primitive's attribute table for `surface_st`,
    // `st` and `uv` in turn. A table that was never asked for any of
    // them is not empty -- it is *absent*, and the lookup dereferences
    // it anyway.
    //
    // Whether the real `DwaBaseMaterial` asks for these, or MoonRay
    // guards the lookup and this build predates the guard, is not
    // knowable from here. What is measurable is that asking is the
    // difference between a render and a segfault.
    mRequiredAttributes.clear();
    mRequiredAttributes.push_back(StandardAttributes::sSt);
    mRequiredAttributes.push_back(StandardAttributes::sSurfaceST);
    mRequiredAttributes.push_back(StandardAttributes::sUv);
    mRequiredAttributes.push_back(StandardAttributes::sNormal);
}

void
DwaBaseMaterial::shade(const scene_rdl2::rdl2::Material* /*self*/,
                       moonray::shading::TLState* /*tls*/,
                       const State& /*state*/,
                       BsdfBuilder& /*bsdfBuilder*/)
{
}
