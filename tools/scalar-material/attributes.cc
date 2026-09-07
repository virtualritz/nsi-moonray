// Copyright notice omitted: this is a probe, not a shipped shader.

#include <scene_rdl2/scene/rdl2/rdl2.h>

using namespace scene_rdl2;

RDL2_DSO_ATTR_DECLARE

    rdl2::AttributeKey<rdl2::Rgb> attrColor;

RDL2_DSO_ATTR_DEFINE(rdl2::Material)

    attrColor = sceneClass.declareAttribute<rdl2::Rgb>(
        "color", rdl2::Rgb(1.0f, 1.0f, 1.0f));
    sceneClass.setMetadata(attrColor, rdl2::SceneClass::sComment,
        "Albedo of the single Lambertian lobe this material adds.");

RDL2_DSO_ATTR_END
