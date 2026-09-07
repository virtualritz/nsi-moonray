/// A stand-in for the `DwaBaseMaterial` MoonRay's `MeshLight` requires.
///
/// **This is not DreamWorks' `DwaBaseMaterial`.** It is a class of that
/// name carrying the one attribute MoonRay binds to, so that a scene
/// with a `MeshLight` in it can be rendered at all.
///
/// `RenderContext::createMeshLightLayer` creates a `DwaBaseMaterial`
/// per mesh light and binds the light's `map_shader` to its `albedo`.
/// `DwaBaseMaterial` is not part of `moonray` -- it ships with
/// `moonshine_dwa` -- so on a `scene_rdl2` + `moonray` build every
/// scene containing a `MeshLight` fails in render prep with
///
/// ```text
/// Error: Couldn't find DSO for 'DwaBaseMaterial' in search path '...'.
/// ```
///
/// Reported as `upstream/moonray-meshlight-needs-moonshine-dwa.md`.
///
/// The material is never shaded: the mesh-light layer exists so the
/// light's geometry can be tessellated and so the map shader's
/// primitive-attribute requests are gathered -- MoonRay's own comment
/// says the material "contains 'map shader' so that it can grab
/// requested primitive attributes" -- and the light samples the `Map`
/// itself. So one bindable colour is the whole contract.

#include <scene_rdl2/scene/rdl2/rdl2.h>

using namespace scene_rdl2;

RDL2_DSO_ATTR_DECLARE

    rdl2::AttributeKey<rdl2::Rgb> attrAlbedo;

RDL2_DSO_ATTR_DEFINE(rdl2::Material)

    attrAlbedo = sceneClass.declareAttribute<rdl2::Rgb>(
        "albedo", rdl2::Rgb(1.0f, 1.0f, 1.0f), rdl2::FLAGS_BINDABLE);
    sceneClass.setMetadata(attrAlbedo, rdl2::SceneClass::sComment,
        "The radiance map a `MeshLight` binds here. Bindable, which is "
        "the only thing MoonRay asks of this class.");

RDL2_DSO_ATTR_END
