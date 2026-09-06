// The C surface over `scene_rdl2`, and later over MoonRay's renderer.
//
// Why it exists: an ɴsɪ backend has to *edit* a scene between frames,
// and a `.rdla` file cannot express an edit. Spawning `moonray` and
// handing it a file forecloses incremental updates, progressive
// delivery and concurrent rendering all at once, because a separate
// process has no `SceneContext` to change and no `RenderContext` to
// snapshot.
//
// Two rules hold everywhere below:
//
//   1. **No C++ exception crosses this boundary.** `SceneObject::set`
//      by name throws `except::KeyError` for an unknown attribute and
//      `except::TypeError` for a mismatched one, and unwinding into
//      Rust is undefined behaviour. Every entry point catches and
//      returns a code.
//   2. **Nothing here refuses a scene.** ɴsɪ always returns an image,
//      so a failing call reports and the caller carries on. That is
//      why these return a code rather than aborting.
//
// Pointers handed out are owned by the `SceneContext` and stay valid
// for its lifetime. `nmr_context_free` is the only destructor.

#ifndef NSI_MOONRAY_SHIM_H
#define NSI_MOONRAY_SHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Return codes. Anything non-zero is reported by the caller and does
// not stop the flush.
#define NMR_OK 0
// No attribute of that name on the object's class.
#define NMR_NO_SUCH_ATTRIBUTE 1
// The attribute exists but is not of the type being written.
#define NMR_TYPE_MISMATCH 2
// A null handle, or a value that is not usable.
#define NMR_BAD_ARGUMENT 3
// Anything rdl2 threw that is neither of the above.
#define NMR_FAILED 4

// rdl2 has exactly two motion timesteps. `NMR_WHOLE_SHUTTER` writes the
// value without one, which sets it for the whole shutter.
#define NMR_WHOLE_SHUTTER (-1)
#define NMR_TIMESTEP_BEGIN 0
#define NMR_TIMESTEP_END 1

typedef struct NmrContext NmrContext;
typedef struct NmrObject NmrObject;

// A scene context. `dso_path` may be null, in which case rdl2's own
// default search is used.
NmrContext* nmr_context_new(const char* dso_path);
void nmr_context_free(NmrContext* context);

// The last error rdl2 produced on this context, as a NUL-terminated
// string, or null. Owned by the context and valid until the next
// failing call.
const char* nmr_context_error(const NmrContext* context);

// Create the object, or return the existing one of that name. Null if
// the class cannot be loaded -- which is the ordinary way a missing DSO
// shows up, so the caller reports it by name.
NmrObject* nmr_object(NmrContext* context, const char* class_name,
                      const char* object_name);

// rdl2 requires every `set` and `set_binding` to happen between these.
int nmr_begin_update(NmrObject* object);
int nmr_end_update(NmrObject* object);

// Scalars. `timestep` is `NMR_WHOLE_SHUTTER` or one of the two.
int nmr_set_bool(NmrObject* o, const char* name, int value, int timestep);
int nmr_set_int(NmrObject* o, const char* name, int32_t value, int timestep);
int nmr_set_long(NmrObject* o, const char* name, int64_t value, int timestep);
int nmr_set_float(NmrObject* o, const char* name, float value, int timestep);
int nmr_set_double(NmrObject* o, const char* name, double value, int timestep);
int nmr_set_string(NmrObject* o, const char* name, const char* value,
                   int timestep);

// Tuples, taken as pointers to the obvious number of components.
int nmr_set_rgb(NmrObject* o, const char* name, const float* v, int timestep);
int nmr_set_rgba(NmrObject* o, const char* name, const float* v, int timestep);
int nmr_set_vec2f(NmrObject* o, const char* name, const float* v, int timestep);
int nmr_set_vec2d(NmrObject* o, const char* name, const double* v, int timestep);
int nmr_set_vec3f(NmrObject* o, const char* name, const float* v, int timestep);
int nmr_set_vec3d(NmrObject* o, const char* name, const double* v, int timestep);
int nmr_set_vec4f(NmrObject* o, const char* name, const float* v, int timestep);
int nmr_set_vec4d(NmrObject* o, const char* name, const double* v, int timestep);

// Matrices, row-major and 16 components, as ɴsɪ stores them and as
// rdl2 prints them.
int nmr_set_mat4f(NmrObject* o, const char* name, const float* v, int timestep);
int nmr_set_mat4d(NmrObject* o, const char* name, const double* v, int timestep);

// A `SceneObject*` attribute. `value` may be null, which is rdl2's
// `undef()` -- and note that MoonRay does not render a `Layer` row
// whose material is undefined, so writing null is a decision.
int nmr_set_object(NmrObject* o, const char* name, NmrObject* value,
                   int timestep);

// Vectors. `count` is the number of *elements*, not of components, so
// a `Vec3f` vector of four takes twelve floats.
int nmr_set_bool_vector(NmrObject* o, const char* name, const int* v,
                        size_t count);
int nmr_set_int_vector(NmrObject* o, const char* name, const int32_t* v,
                       size_t count);
int nmr_set_long_vector(NmrObject* o, const char* name, const int64_t* v,
                        size_t count);
int nmr_set_float_vector(NmrObject* o, const char* name, const float* v,
                         size_t count);
int nmr_set_double_vector(NmrObject* o, const char* name, const double* v,
                          size_t count);
int nmr_set_string_vector(NmrObject* o, const char* name,
                          const char* const* v, size_t count);
int nmr_set_rgb_vector(NmrObject* o, const char* name, const float* v,
                       size_t count);
int nmr_set_vec2f_vector(NmrObject* o, const char* name, const float* v,
                         size_t count);
int nmr_set_vec3f_vector(NmrObject* o, const char* name, const float* v,
                         size_t count);
int nmr_set_vec3d_vector(NmrObject* o, const char* name, const double* v,
                         size_t count);
int nmr_set_vec4f_vector(NmrObject* o, const char* name, const float* v,
                         size_t count);
int nmr_set_mat4f_vector(NmrObject* o, const char* name, const float* v,
                         size_t count);
int nmr_set_mat4d_vector(NmrObject* o, const char* name, const double* v,
                         size_t count);
int nmr_set_object_vector(NmrObject* o, const char* name,
                          NmrObject* const* v, size_t count);

// Bind an attribute to another object -- where an ɴsɪ named shader port
// lands. The attribute keeps its own value alongside the binding, which
// the format oracle settled.
int nmr_set_binding(NmrObject* o, const char* name, NmrObject* target);

// Add a member to a `GeometrySet` or `LightSet`.
int nmr_set_add(NmrObject* set, NmrObject* member);

// One `Layer` row. `part` may be null for the whole geometry, and
// `material` or `light_set` may be null.
int nmr_layer_assign(NmrObject* layer, NmrObject* geometry, const char* part,
                     NmrObject* material, NmrObject* light_set);

// Write the live context out. This is what keeps `.rdla` honest once it
// is no longer the transport: dump the applied scene and diff it
// against what the emitter writes.
int nmr_context_write_ascii(NmrContext* context, const char* path);

// ─── Rendering, in this process ──────────────────────────────────────
//
// MoonRay renders progressively already: `RenderMode::PROGRESSIVE` puts
// samples up as they exist. What it does not do is *push* them -- a
// consumer pulls with `snapshotRenderBuffer` or `snapshotDelta`, and
// `moonray_gui` is a loop around exactly that. ɴsɪ pushes to an output
// driver's callbacks, so the adapter is a snapshot loop on this side.
//
// The one thing that made that impossible before was spawning: a
// separate process has no `RenderContext` to snapshot.
//
// **The renderer owns the scene.** `RenderContext::getSceneContext()`
// hands out a reference to its own, so a scene meant to be rendered is
// built *inside* the renderer rather than built separately and given
// to it. `nmr_render_scene` is that scene, and it must not outlive the
// render context.

typedef struct NmrRender NmrRender;

// How the frame is rendered. `moonray/rendering/rndr/Types.h`.
//
// `BATCH` renders each tile to completion before moving on, which is
// what a file-writing render wants and what makes a viewport look
// frozen. `PROGRESSIVE` puts samples up as they arrive, which is what
// a snapshot loop is for.
#define NMR_MODE_BATCH 0
#define NMR_MODE_PROGRESSIVE 1
// Stops path tracing and renders a simplified frame -- something on
// screen immediately, then converge.
#define NMR_MODE_PROGRESSIVE_FAST 2
// A new frame every n milliseconds, with no refinement between.
#define NMR_MODE_REALTIME 3

// A render context. `threads` of 0 means every core, `mode` one of the
// `NMR_MODE_*` above.
NmrRender* nmr_render_new(const char* dso_path, unsigned threads, int mode);
void nmr_render_free(NmrRender* render);

// The last thing MoonRay complained about, or null.
const char* nmr_render_error(const NmrRender* render);

// The scene to build into. Borrowed from the render context and freed
// with `nmr_context_free`, which knows not to delete what it does not
// own.
NmrContext* nmr_render_scene(NmrRender* render);

// Prepare. Call once, after the scene is built.
int nmr_render_initialize(NmrRender* render);

// Begin rendering. Returns as soon as render prep is done -- the frame
// keeps converging in the background, which is what makes a snapshot
// loop worth having.
int nmr_render_start(NmrRender* render);
int nmr_render_stop(NmrRender* render);

// Where a snapshot loop looks. `ready_for_display` goes true once
// there is something worth showing; `frame_complete` once there is
// nothing more coming.
int nmr_render_is_ready_for_display(const NmrRender* render);
int nmr_render_is_frame_complete(const NmrRender* render);

// Whether the coarse passes are done -- the point at which a
// progressive frame stops looking blocky and starts refining. A
// snapshot loop uses this to decide when the first frame is worth
// showing, ahead of `frame_complete`.
int nmr_render_are_coarse_passes_complete(const NmrRender* render);

// The frame's dimensions, as the renderer resolved them -- which is
// not necessarily what the scene asked for, since `res` scales it.
int nmr_render_resolution(const NmrRender* render, unsigned* width,
                          unsigned* height);

// Copy the current frame into `pixels`, which must hold
// `width * height * 4` floats: MoonRay's render buffer is RGBA float
// per pixel. Returns `NMR_BAD_ARGUMENT` if it is shorter.
int nmr_render_snapshot(NmrRender* render, float* pixels, size_t capacity);

#ifdef __cplusplus
}
#endif

#endif
