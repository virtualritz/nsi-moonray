// Copyright 2026 Moritz Moeller
// SPDX-License-Identifier: MIT OR Apache-2.0 OR Zlib
//
// The `.rdla` format oracle.
//
// Builds small scenes through the real `scene_rdl2` library and writes
// them out with its own `AsciiWriter`. The output is the ground truth
// this backend's emitter is checked against; nothing about the format
// is inferred.
//
// Needs only `scene_rdl2` -- no renderer. The scene classes come from
// rdl2's built-ins and from the DSOs its own test suite builds, so a
// modest machine can capture the oracle.

#include <scene_rdl2/scene/rdl2/rdl2.h>

#include <cstdlib>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>

using namespace scene_rdl2;
using namespace scene_rdl2::rdl2;

namespace {

/// Write one context out, next to the others.
void
capture(const SceneContext& context, const std::string& directory,
        const std::string& name)
{
    AsciiWriter writer(context);
    // Defaults are skipped: what this backend must reproduce is the
    // attributes a scene actually sets, and a full dump of every
    // declared default would bury them.
    writer.setSkipDefaults(true);
    const std::string path = directory + "/" + name + ".rdla";
    writer.toFile(path);
    std::cout << "wrote " << path << '\n';
}

/// Every attribute type, one object. `ExtensiveObject` declares one
/// attribute per `AttributeType`, which makes it the type oracle.
void
types(const std::string& dsoPath, const std::string& outDirectory)
{
    SceneContext context;
    context.setDsoPath(dsoPath);

    SceneObject* object = context.createSceneObject("ExtensiveObject", "/oracle/types");
    SceneObject* other = context.createSceneObject("ExtensiveObject", "/oracle/other");

    SceneObjectVector objects;
    objects.push_back(other);

    {
        SceneObject::UpdateGuard guard(object);
        object->set("bool", false);
        object->set("int", Int(7));
        object->set("long", Long(8));
        object->set("float", 1.5f);
        object->set("double", 2.5);
        object->set("string", String("a string"));
        object->set("rgb", Rgb(0.25f, 0.5f, 0.75f));
        object->set("rgba", Rgba(0.25f, 0.5f, 0.75f, 1.0f));
        object->set("vec2f", Vec2f(9.0f, 8.0f));
        object->set("vec2d", Vec2d(1.0, 2.0));
        object->set("vec3f", Vec3f(9.0f, 8.0f, 7.0f));
        object->set("vec3d", Vec3d(1.0, 2.0, 3.0));
        object->set("vec4f", Vec4f(9.0f, 8.0f, 7.0f, 6.0f));
        object->set("vec4d", Vec4d(1.0, 2.0, 3.0, 4.0));
        object->set("mat4f", Mat4f(1.0f, 0.0f, 0.0f, 0.0f,
                                   0.0f, 1.0f, 0.0f, 0.0f,
                                   0.0f, 0.0f, 1.0f, 0.0f,
                                   4.0f, 5.0f, 6.0f, 1.0f));
        object->set("mat4d", Mat4d(1.0, 0.0, 0.0, 0.0,
                                   0.0, 1.0, 0.0, 0.0,
                                   0.0, 0.0, 1.0, 0.0,
                                   4.0, 5.0, 6.0, 1.0));
        object->set("scene_object", other);

        object->set("bool_vector", BoolVector{false, true, false});
        object->set("int_vector", IntVector{1, 2, 3});
        object->set("long_vector", LongVector{4, 5});
        // Awkward values on purpose: this is where the number
        // formatting rdl2 uses gets pinned down, rather than
        // guessed at from the round ones.
        object->set("float_vector", FloatVector{0.1f, 1e20f, 1e-7f, -1.25f, 1234567.0f});
        object->set("double_vector", DoubleVector{0.1, 1e300, 1e-7, -2.5, 1234567890123.0});
        object->set("string_vector", StringVector{"one", "two"});
        object->set("rgb_vector", RgbVector{Rgb(1.0f, 0.0f, 0.0f), Rgb(0.0f, 1.0f, 0.0f)});
        object->set("rgba_vector", RgbaVector{Rgba(1.0f, 0.0f, 0.0f, 1.0f)});
        object->set("vec2f_vector", Vec2fVector{Vec2f(1.0f, 2.0f)});
        object->set("vec3f_vector", Vec3fVector{Vec3f(9.0f, 8.0f, 7.0f)});
        object->set("vec2d_vector", Vec2dVector{Vec2d(9.0, 8.0)});
        object->set("vec3d_vector", Vec3dVector{Vec3d(9.0, 8.0, 7.0)});
        object->set("vec4d_vector", Vec4dVector{Vec4d(9.0, 8.0, 7.0, 6.0)});
        object->set("mat4d_vector", Mat4dVector{Mat4d(2.0, 0.0, 0.0, 0.0,
                                                      0.0, 2.0, 0.0, 0.0,
                                                      0.0, 0.0, 2.0, 0.0,
                                                      0.0, 0.0, 0.0, 1.0)});
        object->set("vec4f_vector", Vec4fVector{Vec4f(1.0f, 2.0f, 3.0f, 4.0f)});
        object->set("mat4f_vector", Mat4fVector{Mat4f(1.0f, 0.0f, 0.0f, 0.0f,
                                                      0.0f, 1.0f, 0.0f, 0.0f,
                                                      0.0f, 0.0f, 1.0f, 0.0f,
                                                      0.0f, 0.0f, 0.0f, 1.0f)});
        object->set("scene_object_vector", objects);
    }

    capture(context, outDirectory, "types");
}

/// A scene shaped like the one this backend has to emit: geometry with
/// a world transform, a material bound through a `Layer`, a camera, a
/// render output and the scene variables.
void
scene(const std::string& dsoPath, const std::string& outDirectory)
{
    SceneContext context;
    context.setDsoPath(dsoPath);

    Geometry* geometry =
        context.createSceneObject("FakeTeapot", "/oracle/geometry")->asA<Geometry>();
    Material* material =
        context.createSceneObject("FakeMaterial", "/oracle/material")->asA<Material>();
    Light* light = context.createSceneObject("FakeLight", "/oracle/light")->asA<Light>();
    LightSet* lights =
        context.createSceneObject("LightSet", "/oracle/lights")->asA<LightSet>();
    Layer* layer = context.createSceneObject("Layer", "/oracle/layer")->asA<Layer>();
    GeometrySet* geometries =
        context.createSceneObject("GeometrySet", "/oracle/geometries")->asA<GeometrySet>();
    SceneObject* output = context.createSceneObject("RenderOutput", "/oracle/beauty");

    {
        SceneObject::UpdateGuard guard(geometry);
        // `node_xform` is `Node`'s Mat4d. This is where a resolved
        // world transform lands.
        geometry->set(Node::sNodeXformKey,
                      Mat4d(1.0, 0.0, 0.0, 0.0,
                            0.0, 1.0, 0.0, 0.0,
                            0.0, 0.0, 1.0, 0.0,
                            1.0, 2.0, 3.0, 1.0));
    }

    {
        SceneObject::UpdateGuard guard(lights);
        lights->add(light);
    }

    {
        SceneObject::UpdateGuard guard(geometries);
        geometries->add(geometry);
    }

    {
        SceneObject::UpdateGuard guard(layer);
        layer->assign(geometry, "", material, lights);
    }

    {
        SceneObject::UpdateGuard guard(output);
        output->set("file_name", String("beauty.exr"));
    }

    SceneVariables& variables = context.getSceneVariables();
    {
        SceneObject::UpdateGuard guard(&variables);
        variables.set(SceneVariables::sImageWidth, Int(320));
        variables.set(SceneVariables::sImageHeight, Int(240));
        variables.set(SceneVariables::sLayer, static_cast<SceneObject*>(layer));
    }

    capture(context, outDirectory, "scene");
}

/// Motion blur, both of the shapes this backend needs: a blurred
/// transform, and a blurred vector attribute standing in for deforming
/// vertices.
void
blur(const std::string& dsoPath, const std::string& outDirectory)
{
    SceneContext context;
    context.setDsoPath(dsoPath);

    Geometry* geometry =
        context.createSceneObject("FakeTeapot", "/oracle/moving")->asA<Geometry>();
    SceneObject* object =
        context.createSceneObject("ExtensiveObject", "/oracle/blurred");

    {
        SceneObject::UpdateGuard guard(geometry);
        geometry->set(Node::sNodeXformKey,
                      Mat4d(1.0, 0.0, 0.0, 0.0,
                            0.0, 1.0, 0.0, 0.0,
                            0.0, 0.0, 1.0, 0.0,
                            0.0, 0.0, 0.0, 1.0),
                      TIMESTEP_BEGIN);
        geometry->set(Node::sNodeXformKey,
                      Mat4d(1.0, 0.0, 0.0, 0.0,
                            0.0, 1.0, 0.0, 0.0,
                            0.0, 0.0, 1.0, 0.0,
                            5.0, 0.0, 0.0, 1.0),
                      TIMESTEP_END);
    }

    {
        SceneObject::UpdateGuard guard(object);
        object->set("float", 0.0f, TIMESTEP_BEGIN);
        object->set("float", 1.0f, TIMESTEP_END);
        object->set("vec3f", Vec3f(0.0f, 0.0f, 0.0f), TIMESTEP_BEGIN);
        object->set("vec3f", Vec3f(0.0f, 1.0f, 0.0f), TIMESTEP_END);
    }

    capture(context, outDirectory, "blur");
}

/// Negative zero, on its own.
///
/// rdl2's writer prints `-0`, and its reader turns that back into `0` —
/// so this one scene does not survive `verify`. It is captured
/// separately for exactly that reason: the emitter has to match the
/// writer, and the asymmetry is upstream's, not something to paper over
/// by rounding it away here.
void
signedZero(const std::string& dsoPath, const std::string& outDirectory)
{
    SceneContext context;
    context.setDsoPath(dsoPath);

    SceneObject* object =
        context.createSceneObject("ExtensiveObject", "/oracle/signed_zero");

    {
        SceneObject::UpdateGuard guard(object);
        object->set("float", -0.0f);
        object->set("double", -0.0);
    }

    capture(context, outDirectory, "signed_zero");
}

/// An attribute bound to another object. ɴsɪ's named shader ports land
/// here, so the emitted syntax matters.
void
binding(const std::string& dsoPath, const std::string& outDirectory)
{
    SceneContext context;
    context.setDsoPath(dsoPath);

    SceneObject* object = context.createSceneObject("ExtensiveObject", "/oracle/bound");
    SceneObject* source = context.createSceneObject("FakeMaterial", "/oracle/source");

    {
        SceneObject::UpdateGuard guard(object);
        object->setBinding("string", source);
    }

    capture(context, outDirectory, "binding");
}

/// Read a `.rdla` file back through rdl2's own `AsciiReader` and write
/// it out again with `AsciiWriter`, then compare.
///
/// Byte equality against a captured file proves this backend spells the
/// format the way rdl2 does. Only this proves rdl2 *accepts* what it
/// spells -- a file can be written correctly enough to diff clean and
/// still be rejected, and one that rdl2 refuses renders nothing at all.
bool
verify(const std::string& dsoPath, const std::string& path)
{
    std::ifstream input(path);
    if (!input) {
        std::cerr << "cannot read " << path << '\n';
        return false;
    }

    std::ostringstream original;
    original << input.rdbuf();

    SceneContext context;
    context.setDsoPath(dsoPath);

    AsciiReader reader(context);
    reader.fromString(original.str(), "@" + path);

    AsciiWriter writer(context);
    writer.setSkipDefaults(true);
    const std::string round = writer.toString();

    if (round != original.str()) {
        std::cerr << path << ": round-trip differs\n--- read back ---\n"
                  << round << "--- original ---\n"
                  << original.str();
        return false;
    }

    std::cout << "round-trips " << path << '\n';
    return true;
}

void
usage()
{
    std::cerr << "usage: oracle capture <rdl2-dso-path> <output-directory>\n"
              << "       oracle verify  <rdl2-dso-path> <file.rdla>...\n";
}

} // namespace

int
main(int argc, char** argv)
{
    if (argc < 3) {
        usage();
        return EXIT_FAILURE;
    }

    const std::string mode = argv[1];
    const std::string dsoPath = argv[2];

    try {
        if (mode == "capture") {
            if (argc != 4) {
                usage();
                return EXIT_FAILURE;
            }

            const std::string outDirectory = argv[3];
            types(dsoPath, outDirectory);
            scene(dsoPath, outDirectory);
            blur(dsoPath, outDirectory);
            binding(dsoPath, outDirectory);
            signedZero(dsoPath, outDirectory);
            return EXIT_SUCCESS;
        }

        if (mode == "verify") {
            if (argc < 4) {
                usage();
                return EXIT_FAILURE;
            }

            bool ok = true;
            for (int index = 3; index < argc; ++index) {
                ok = verify(dsoPath, argv[index]) && ok;
            }
            return ok ? EXIT_SUCCESS : EXIT_FAILURE;
        }

        usage();
        return EXIT_FAILURE;
    } catch (const std::exception& error) {
        std::cerr << "oracle failed: " << error.what() << '\n';
        return EXIT_FAILURE;
    }
}
