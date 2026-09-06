// The `scene_rdl2` half of the shim. See `nsi_moonray_shim.h` for the
// two rules every entry point here obeys.

#include "nsi_moonray_shim.h"

#include <scene_rdl2/scene/rdl2/rdl2.h>
#include <scene_rdl2/scene/rdl2/AsciiWriter.h>
#include <scene_rdl2/common/except/exceptions.h>

#include <cstring>
#include <string>
#include <vector>

namespace rdl2 = scene_rdl2::rdl2;
namespace math = scene_rdl2::math;

// The context, plus the last error. rdl2 reports by throwing, and the
// codes this returns are deliberately coarse -- the message is what a
// person needs, so it is kept alongside rather than encoded.
//
// The scene is a *pointer* rather than a member because MoonRay's
// `RenderContext` owns one and hands out a reference to it
// (`getSceneContext`, "only call this when not rendering"). So a scene
// built for rendering has to be built inside the renderer's own
// context, not built separately and handed over. `owned` says which
// kind this is; only a standalone one is deleted.
struct NmrContext {
    rdl2::SceneContext* context;
    bool owned;
    std::string error;
};

// Used by `render.cc` to wrap a `RenderContext`'s own scene.
NmrContext* nmr_context_borrow(rdl2::SceneContext* scene)
{
    auto* wrapper = new NmrContext{scene, false, {}};
    return wrapper;
}

extern "C" {

NmrContext* nmr_context_new(const char* dso_path)
{
    try {
        auto* wrapper =
            new NmrContext{new rdl2::SceneContext(), true, {}};
        if (dso_path != nullptr && dso_path[0] != '\0') {
            wrapper->context->setDsoPath(dso_path);
        }
        return wrapper;
    } catch (...) {
        return nullptr;
    }
}

void nmr_context_free(NmrContext* context)
{
    if (context == nullptr) {
        return;
    }
    if (context->owned) {
        delete context->context;
    }
    delete context;
}

const char* nmr_context_error(const NmrContext* context)
{
    if (context == nullptr || context->error.empty()) {
        return nullptr;
    }
    return context->error.c_str();
}

NmrObject* nmr_object(NmrContext* context, const char* class_name,
                      const char* object_name)
{
    if (context == nullptr || class_name == nullptr
        || object_name == nullptr) {
        return nullptr;
    }
    try {
        // `createSceneObject` is create-or-get, which is what makes a
        // second flush of the same scene idempotent rather than an
        // error -- and what an incremental apply relies on.
        return reinterpret_cast<NmrObject*>(
            context->context->createSceneObject(class_name, object_name));
    } catch (const std::exception& error) {
        context->error = std::string(class_name) + " \"" + object_name
            + "\": " + error.what();
        return nullptr;
    } catch (...) {
        context->error = std::string("creating ") + class_name;
        return nullptr;
    }
}

} // extern "C"

namespace {

rdl2::SceneObject* object_of(NmrObject* handle)
{
    return reinterpret_cast<rdl2::SceneObject*>(handle);
}

rdl2::AttributeTimestep timestep_of(int timestep)
{
    return timestep == NMR_TIMESTEP_END ? rdl2::TIMESTEP_END
                                        : rdl2::TIMESTEP_BEGIN;
}

// Every setter funnels through here, so the exception rule is stated
// once rather than in forty places. `NMR_NO_SUCH_ATTRIBUTE` and
// `NMR_TYPE_MISMATCH` are separated because they mean different things
// to whoever reads the report: the first is a mapping this backend has
// not written, the second is one it wrote wrongly.
template <typename Body>
int guarded(NmrObject* handle, Body body)
{
    if (handle == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    try {
        body();
        return NMR_OK;
    } catch (const scene_rdl2::except::KeyError&) {
        return NMR_NO_SUCH_ATTRIBUTE;
    } catch (const scene_rdl2::except::TypeError&) {
        return NMR_TYPE_MISMATCH;
    } catch (...) {
        return NMR_FAILED;
    }
}

// A scalar, written either for the whole shutter or at one timestep.
template <typename T>
int set_scalar(NmrObject* handle, const char* name, const T& value,
               int timestep)
{
    if (name == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    return guarded(handle, [&] {
        if (timestep == NMR_WHOLE_SHUTTER) {
            object_of(handle)->set(std::string(name), value);
        } else {
            object_of(handle)->set(std::string(name), value,
                                   timestep_of(timestep));
        }
    });
}

// `rdl2::BoolVector` is a `std::deque<bool>` -- rdl2 avoids
// `std::vector<bool>` and its proxy references -- and a deque has no
// `reserve`. Everything else does, and a vertex list is long enough
// for it to matter.
template <typename Vector>
void reserve(Vector& values, size_t count)
{
    values.reserve(count);
}

// The one that has none.
inline void reserve(rdl2::BoolVector&, size_t) {}

// A vector attribute, built from a flat buffer of components.
template <typename Vector, typename Element, typename Make>
int set_vector(NmrObject* handle, const char* name, size_t count, Make make)
{
    if (name == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    return guarded(handle, [&] {
        Vector values;
        reserve(values, count);
        for (size_t i = 0; i < count; ++i) {
            values.push_back(make(i));
        }
        object_of(handle)->set(std::string(name), values);
    });
}

} // namespace

extern "C" {

int nmr_begin_update(NmrObject* object)
{
    return guarded(object, [&] { object_of(object)->beginUpdate(); });
}

int nmr_end_update(NmrObject* object)
{
    return guarded(object, [&] { object_of(object)->endUpdate(); });
}

int nmr_set_bool(NmrObject* o, const char* name, int value, int timestep)
{
    return set_scalar<rdl2::Bool>(o, name, value != 0, timestep);
}

int nmr_set_int(NmrObject* o, const char* name, int32_t value, int timestep)
{
    return set_scalar<rdl2::Int>(o, name, value, timestep);
}

int nmr_set_long(NmrObject* o, const char* name, int64_t value, int timestep)
{
    return set_scalar<rdl2::Long>(o, name, value, timestep);
}

int nmr_set_float(NmrObject* o, const char* name, float value, int timestep)
{
    return set_scalar<rdl2::Float>(o, name, value, timestep);
}

int nmr_set_double(NmrObject* o, const char* name, double value, int timestep)
{
    return set_scalar<rdl2::Double>(o, name, value, timestep);
}

int nmr_set_string(NmrObject* o, const char* name, const char* value,
                   int timestep)
{
    if (value == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    return set_scalar<rdl2::String>(o, name, rdl2::String(value), timestep);
}

int nmr_set_rgb(NmrObject* o, const char* name, const float* v, int timestep)
{
    if (v == nullptr) return NMR_BAD_ARGUMENT;
    return set_scalar<rdl2::Rgb>(o, name, rdl2::Rgb(v[0], v[1], v[2]),
                                 timestep);
}

int nmr_set_rgba(NmrObject* o, const char* name, const float* v, int timestep)
{
    if (v == nullptr) return NMR_BAD_ARGUMENT;
    return set_scalar<rdl2::Rgba>(o, name, rdl2::Rgba(v[0], v[1], v[2], v[3]),
                                  timestep);
}

int nmr_set_vec2f(NmrObject* o, const char* name, const float* v, int timestep)
{
    if (v == nullptr) return NMR_BAD_ARGUMENT;
    return set_scalar<rdl2::Vec2f>(o, name, rdl2::Vec2f(v[0], v[1]), timestep);
}

int nmr_set_vec2d(NmrObject* o, const char* name, const double* v, int timestep)
{
    if (v == nullptr) return NMR_BAD_ARGUMENT;
    return set_scalar<rdl2::Vec2d>(o, name, rdl2::Vec2d(v[0], v[1]), timestep);
}

int nmr_set_vec3f(NmrObject* o, const char* name, const float* v, int timestep)
{
    if (v == nullptr) return NMR_BAD_ARGUMENT;
    return set_scalar<rdl2::Vec3f>(o, name, rdl2::Vec3f(v[0], v[1], v[2]),
                                   timestep);
}

int nmr_set_vec3d(NmrObject* o, const char* name, const double* v, int timestep)
{
    if (v == nullptr) return NMR_BAD_ARGUMENT;
    return set_scalar<rdl2::Vec3d>(o, name, rdl2::Vec3d(v[0], v[1], v[2]),
                                   timestep);
}

int nmr_set_vec4f(NmrObject* o, const char* name, const float* v, int timestep)
{
    if (v == nullptr) return NMR_BAD_ARGUMENT;
    return set_scalar<rdl2::Vec4f>(
        o, name, rdl2::Vec4f(v[0], v[1], v[2], v[3]), timestep);
}

int nmr_set_vec4d(NmrObject* o, const char* name, const double* v, int timestep)
{
    if (v == nullptr) return NMR_BAD_ARGUMENT;
    return set_scalar<rdl2::Vec4d>(
        o, name, rdl2::Vec4d(v[0], v[1], v[2], v[3]), timestep);
}

int nmr_set_mat4f(NmrObject* o, const char* name, const float* v, int timestep)
{
    if (v == nullptr) return NMR_BAD_ARGUMENT;
    return set_scalar<rdl2::Mat4f>(
        o, name,
        rdl2::Mat4f(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8],
                    v[9], v[10], v[11], v[12], v[13], v[14], v[15]),
        timestep);
}

int nmr_set_mat4d(NmrObject* o, const char* name, const double* v, int timestep)
{
    if (v == nullptr) return NMR_BAD_ARGUMENT;
    return set_scalar<rdl2::Mat4d>(
        o, name,
        rdl2::Mat4d(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8],
                    v[9], v[10], v[11], v[12], v[13], v[14], v[15]),
        timestep);
}

int nmr_set_object(NmrObject* o, const char* name, NmrObject* value,
                   int timestep)
{
    if (name == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    return guarded(o, [&] {
        rdl2::SceneObject* target = object_of(value);
        if (timestep == NMR_WHOLE_SHUTTER) {
            object_of(o)->set(std::string(name), target);
        } else {
            object_of(o)->set(std::string(name), target,
                              timestep_of(timestep));
        }
    });
}

int nmr_set_bool_vector(NmrObject* o, const char* name, const int* v,
                        size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::BoolVector, rdl2::Bool>(
        o, name, count, [&](size_t i) { return v[i] != 0; });
}

int nmr_set_int_vector(NmrObject* o, const char* name, const int32_t* v,
                       size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::IntVector, rdl2::Int>(
        o, name, count, [&](size_t i) { return rdl2::Int(v[i]); });
}

int nmr_set_long_vector(NmrObject* o, const char* name, const int64_t* v,
                        size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::LongVector, rdl2::Long>(
        o, name, count, [&](size_t i) { return rdl2::Long(v[i]); });
}

int nmr_set_float_vector(NmrObject* o, const char* name, const float* v,
                         size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::FloatVector, rdl2::Float>(
        o, name, count, [&](size_t i) { return v[i]; });
}

int nmr_set_double_vector(NmrObject* o, const char* name, const double* v,
                          size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::DoubleVector, rdl2::Double>(
        o, name, count, [&](size_t i) { return v[i]; });
}

int nmr_set_string_vector(NmrObject* o, const char* name,
                          const char* const* v, size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::StringVector, rdl2::String>(
        o, name, count, [&](size_t i) {
            return rdl2::String(v[i] == nullptr ? "" : v[i]);
        });
}

int nmr_set_rgb_vector(NmrObject* o, const char* name, const float* v,
                       size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::RgbVector, rdl2::Rgb>(
        o, name, count, [&](size_t i) {
            return rdl2::Rgb(v[i * 3], v[i * 3 + 1], v[i * 3 + 2]);
        });
}

int nmr_set_vec2f_vector(NmrObject* o, const char* name, const float* v,
                         size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::Vec2fVector, rdl2::Vec2f>(
        o, name, count,
        [&](size_t i) { return rdl2::Vec2f(v[i * 2], v[i * 2 + 1]); });
}

int nmr_set_vec3f_vector(NmrObject* o, const char* name, const float* v,
                         size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::Vec3fVector, rdl2::Vec3f>(
        o, name, count, [&](size_t i) {
            return rdl2::Vec3f(v[i * 3], v[i * 3 + 1], v[i * 3 + 2]);
        });
}

int nmr_set_vec3d_vector(NmrObject* o, const char* name, const double* v,
                         size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::Vec3dVector, rdl2::Vec3d>(
        o, name, count, [&](size_t i) {
            return rdl2::Vec3d(v[i * 3], v[i * 3 + 1], v[i * 3 + 2]);
        });
}

int nmr_set_vec4f_vector(NmrObject* o, const char* name, const float* v,
                         size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::Vec4fVector, rdl2::Vec4f>(
        o, name, count, [&](size_t i) {
            return rdl2::Vec4f(v[i * 4], v[i * 4 + 1], v[i * 4 + 2],
                               v[i * 4 + 3]);
        });
}

int nmr_set_mat4f_vector(NmrObject* o, const char* name, const float* v,
                         size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::Mat4fVector, rdl2::Mat4f>(
        o, name, count, [&](size_t i) {
            const float* m = v + i * 16;
            return rdl2::Mat4f(m[0], m[1], m[2], m[3], m[4], m[5], m[6],
                               m[7], m[8], m[9], m[10], m[11], m[12], m[13],
                               m[14], m[15]);
        });
}

int nmr_set_mat4d_vector(NmrObject* o, const char* name, const double* v,
                         size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::Mat4dVector, rdl2::Mat4d>(
        o, name, count, [&](size_t i) {
            const double* m = v + i * 16;
            return rdl2::Mat4d(m[0], m[1], m[2], m[3], m[4], m[5], m[6],
                               m[7], m[8], m[9], m[10], m[11], m[12], m[13],
                               m[14], m[15]);
        });
}

int nmr_set_object_vector(NmrObject* o, const char* name,
                          NmrObject* const* v, size_t count)
{
    if (v == nullptr && count > 0) return NMR_BAD_ARGUMENT;
    return set_vector<rdl2::SceneObjectVector, rdl2::SceneObject*>(
        o, name, count, [&](size_t i) { return object_of(v[i]); });
}

int nmr_set_binding(NmrObject* o, const char* name, NmrObject* target)
{
    if (name == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    return guarded(o, [&] {
        object_of(o)->setBinding(std::string(name), object_of(target));
    });
}

int nmr_set_add(NmrObject* set, NmrObject* member)
{
    if (member == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    return guarded(set, [&] {
        rdl2::SceneObject* object = object_of(set);
        if (auto* geometries = object->asA<rdl2::GeometrySet>()) {
            geometries->add(object_of(member)->asA<rdl2::Geometry>());
        } else if (auto* lights = object->asA<rdl2::LightSet>()) {
            lights->add(object_of(member)->asA<rdl2::Light>());
        } else {
            throw scene_rdl2::except::TypeError("not a set");
        }
    });
}

int nmr_layer_assign(NmrObject* layer, NmrObject* geometry, const char* part,
                     NmrObject* material, NmrObject* light_set)
{
    if (geometry == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    return guarded(layer, [&] {
        rdl2::Layer* target = object_of(layer)->asA<rdl2::Layer>();
        if (target == nullptr) {
            throw scene_rdl2::except::TypeError("not a Layer");
        }
        target->assign(
            object_of(geometry)->asA<rdl2::Geometry>(),
            rdl2::String(part == nullptr ? "" : part),
            material == nullptr ? nullptr
                                : object_of(material)->asA<rdl2::Material>(),
            light_set == nullptr
                ? nullptr
                : object_of(light_set)->asA<rdl2::LightSet>());
    });
}

int nmr_context_write_ascii(NmrContext* context, const char* path)
{
    if (context == nullptr || path == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    try {
        rdl2::AsciiWriter writer(*context->context);
        writer.toFile(path);
        return NMR_OK;
    } catch (const std::exception& error) {
        context->error = error.what();
        return NMR_FAILED;
    } catch (...) {
        return NMR_FAILED;
    }
}

} // extern "C"
