/// The `Osl` material's attributes.
///
/// **Two strings, not a parameter list.** rdl2 declares a class's
/// attributes once, statically, so one class serving every OSL shader
/// in existence cannot have an attribute per parameter. OSL already
/// solved this: `ShadingSystem::ShaderGroupBegin` takes a *group
/// specification* -- layers, parameter values and connections, as
/// text. So the whole ɴsɪ shader network arrives in `group_spec` and
/// the flush's job is a text transformation.
/// `specs/003-osl/research.md` O6.

#include <scene_rdl2/scene/rdl2/rdl2.h>

using namespace scene_rdl2;

/// The lobe labels this class can name.
///
/// MoonRay's material AOVs and light-path expressions key off an
/// integer per lobe, indexing a **static array declared on the scene
/// class** and read back at render prep. OSL's side is a string --
/// `"label"` is a keyword parameter the renderer registers, not part
/// of the language -- so the two meet here, and the table has to be
/// fixed because the class is one and the shaders are many.
///
/// The vocabulary is the conventional one. A label an OSL shader uses
/// that is not here cannot be represented, and the material says so
/// rather than dropping it: an LPE naming a label that never
/// registered renders black, which reads as a lighting bug.
/// `specs/003-osl/research.md` O5.
static const char* labels[] = {
    "diffuse",       //  1
    "specular",      //  2
    "transmission",  //  3
    "subsurface",    //  4
    "sheen",         //  5
    "coat",          //  6
    "emission",      //  7
    "hair",          //  8
    nullptr,
};

RDL2_DSO_ATTR_DECLARE

    rdl2::AttributeKey<rdl2::String> attrGroupSpec;
    rdl2::AttributeKey<rdl2::String> attrGroupName;
    rdl2::AttributeKey<rdl2::String> attrSearchPath;

RDL2_DSO_ATTR_DEFINE(rdl2::Material)

    attrGroupSpec = sceneClass.declareAttribute<rdl2::String>(
        "group_spec", "");
    sceneClass.setMetadata(attrGroupSpec, rdl2::SceneClass::sComment,
        "An OSL shader group specification: `param <type> <name> <value> ;`, "
        "`shader <shader> <layer> ;` and `connect <layer>.<param> "
        "<layer>.<param> ;`, separated by semicolons or commas. This is a "
        "whole shader network, which is how an ɴsɪ shader graph crosses "
        "into a class whose attributes are declared statically.");

    attrGroupName = sceneClass.declareAttribute<rdl2::String>(
        "group_name", "");
    sceneClass.setMetadata(attrGroupName, rdl2::SceneClass::sComment,
        "A name for the group, used in OSL's own diagnostics. The ɴsɪ "
        "handle of the shader the network is rooted at.");

    attrSearchPath = sceneClass.declareAttribute<rdl2::String>(
        "search_path", "");
    sceneClass.setMetadata(attrSearchPath, rdl2::SceneClass::sComment,
        "Where to find compiled `.oso` shaders. Empty reads "
        "$OSL_SHADER_PATH, then the working directory.");

    sceneClass.declareDataPtr("labels", labels);

RDL2_DSO_ATTR_END
