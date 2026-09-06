// The MoonRay half of the shim: a renderer in this process.
//
// See `nsi_moonray_shim.h` for why pulling is what MoonRay offers and
// what that means for an ɴsɪ output driver. The two rules from
// `scene.cc` hold here too: no exception crosses the boundary, and
// nothing refuses a scene.

#include "nsi_moonray_shim.h"

#include <moonray/rendering/rndr/RenderContext.h>
#include <moonray/rendering/rndr/RenderOptions.h>
#include <scene_rdl2/common/fb_util/FbTypes.h>
#include <scene_rdl2/scene/rdl2/rdl2.h>

#include <atomic>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>

namespace rndr = moonray::rndr;
namespace rdl2 = scene_rdl2::rdl2;

// Declared in `scene.cc`: wraps a scene this does not own.
NmrContext* nmr_context_borrow(rdl2::SceneContext* scene);

struct NmrRender {
    // `RenderContext` takes its options by non-const reference and
    // keeps using them, so they outlive it here rather than being a
    // temporary at the construction site.
    rndr::RenderOptions options;
    std::unique_ptr<rndr::RenderContext> context;
    scene_rdl2::fb_util::RenderBuffer buffer;
    std::string error;
    bool initialized = false;
    bool rendering = false;
};

namespace {

// MoonRay has process-wide state that must be up before any
// `RenderContext` exists: `initGlobalDriver` starts `ProcKeeper`, the
// `AffinityManager`, the render driver's thread-local pools and the
// image-write driver. `moonray`'s own `main` calls it and a library
// consumer has to as well.
//
// Skipping it does not fail cleanly. `RenderContext::initialize` walks
// into `RenderStats::logHardwareConfiguration`, which asks
// `AffinityManager::get()` for a manager that was never made, and the
// process dies with a SIGSEGV inside `setupLogInfo` -- no message, no
// exception to catch, and a backtrace that points at logging rather
// than at the missing init.
//
// Once per process, so under a `once_flag`: a second call would remake
// global state a live renderer is already using.
std::once_flag global_driver_once;

// **One renderer per process.** MoonRay's driver state is global --
// `initGlobalDriver` sets up thread-local pools, the affinity manager
// and the image-write driver for the process, not for a context -- so a
// second live `RenderContext` shares state the first is using.
//
// It does not fail politely. Two of them in one process abort in the
// allocator (`munmap_chunk(): invalid pointer`), which points nowhere
// near the cause. Refusing the second is the honest answer, and it is
// what an application hitting this needs to be told.
//
// Sequential use is fine: create, render, drop, create again.
std::atomic<bool> renderer_live{false};

void ensure_global_driver(const moonray::rndr::RenderOptions& options)
{
    std::call_once(global_driver_once,
                   [&] { rndr::initGlobalDriver(options); });
}

template <typename Body>
int guarded(NmrRender* render, Body body)
{
    if (render == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    try {
        return body();
    } catch (const std::exception& error) {
        render->error = error.what();
        return NMR_FAILED;
    } catch (...) {
        return NMR_FAILED;
    }
}

} // namespace

extern "C" {

namespace {

rndr::RenderMode mode_of(int mode)
{
    switch (mode) {
    case NMR_MODE_PROGRESSIVE:
        return rndr::RenderMode::PROGRESSIVE;
    case NMR_MODE_PROGRESSIVE_FAST:
        return rndr::RenderMode::PROGRESSIVE_FAST;
    case NMR_MODE_REALTIME:
        return rndr::RenderMode::REALTIME;
    default:
        return rndr::RenderMode::BATCH;
    }
}

} // namespace

NmrRender* nmr_render_new(const char* dso_path, unsigned threads, int mode)
{
    bool expected = false;
    if (!renderer_live.compare_exchange_strong(expected, true)) {
        // A second live renderer. See `renderer_live`.
        return nullptr;
    }

    try {
        auto* render = new NmrRender();
        if (dso_path != nullptr && dso_path[0] != '\0') {
            render->options.setDsoPath(dso_path);
        }
        if (threads > 0) {
            render->options.setThreads(threads);
        }
        // No scene *files*: the whole point is that the scene is built
        // in memory. `RenderContext` is happy with an empty list and
        // gives us its own `SceneContext` to fill.
        render->options.setSceneFiles({});
        // Set before `initGlobalDriver`: it reads the mode to decide
        // whether to size the thread-local pools for realtime.
        render->options.setRenderMode(mode_of(mode));

        ensure_global_driver(render->options);

        std::stringstream messages;
        render->context =
            std::make_unique<rndr::RenderContext>(render->options, &messages);
        return render;
    } catch (...) {
        renderer_live.store(false);
        return nullptr;
    }
}

void nmr_render_free(NmrRender* render)
{
    if (render == nullptr) {
        return;
    }
    // Leaving a frame running past the destructor is a crash rather
    // than a leak, and an application that drops a viewport mid-render
    // is the ordinary way to get there.
    if (render->rendering && render->context) {
        try {
            render->context->stopFrame();
        } catch (...) {
        }
    }
    delete render;
    renderer_live.store(false);
}

const char* nmr_render_error(const NmrRender* render)
{
    if (render == nullptr || render->error.empty()) {
        return nullptr;
    }
    return render->error.c_str();
}

NmrContext* nmr_render_scene(NmrRender* render)
{
    if (render == nullptr || !render->context) {
        return nullptr;
    }
    try {
        return nmr_context_borrow(&render->context->getSceneContext());
    } catch (...) {
        return nullptr;
    }
}

int nmr_render_initialize(NmrRender* render)
{
    return guarded(render, [&] {
        if (render->initialized) {
            return NMR_OK;
        }
        // **A scene with no camera crashes MoonRay.**
        // `RenderContext::initialize` does
        //
        //     std::vector<const Camera*> cameras = getActiveCameras();
        //     initActiveCamera(cameras[0]);
        //
        // and `operator[]` on an empty vector is undefined behaviour,
        // not an exception -- so the `catch (KeyError&)` wrapped around
        // it never fires and the process dies with a SIGSEGV inside
        // `initialize`. In a renderer loaded by `dlopen` that takes the
        // host application down with it.
        //
        // An ɴsɪ scene with no `perspectivecamera` connected is legal
        // to record and a perfectly ordinary thing to hand over. So the
        // check belongs here, ahead of the call.
        if (render->context->getSceneContext().getActiveCameras().empty()) {
            render->error =
                "the scene has no active camera; MoonRay reads "
                "cameras[0] of an empty list and would crash";
            return NMR_FAILED;
        }

        std::stringstream messages;
        render->context->initialize(messages);
        render->initialized = true;
        return NMR_OK;
    });
}

int nmr_render_scene_updated(NmrRender* render)
{
    return guarded(render, [&] {
        render->context->setSceneUpdated();
        return NMR_OK;
    });
}

int nmr_render_start(NmrRender* render)
{
    return guarded(render, [&] {
        if (!render->initialized) {
            render->error = "start before initialize";
            return NMR_FAILED;
        }
        // `startFrame` returns once render *prep* is done; the frame
        // goes on converging behind it, which is what a snapshot loop
        // is for.
        const auto result = render->context->startFrame();
        render->rendering = true;
        return result == rndr::RenderContext::RP_RESULT::FINISHED
            ? NMR_OK
            : NMR_FAILED;
    });
}

int nmr_render_stop(NmrRender* render)
{
    return guarded(render, [&] {
        if (render->rendering) {
            render->context->stopFrame();
            render->rendering = false;
        }
        return NMR_OK;
    });
}

int nmr_render_is_ready_for_display(const NmrRender* render)
{
    if (render == nullptr || !render->context) {
        return 0;
    }
    try {
        return render->context->isFrameReadyForDisplay() ? 1 : 0;
    } catch (...) {
        return 0;
    }
}

int nmr_render_are_coarse_passes_complete(const NmrRender* render)
{
    if (render == nullptr || !render->context) {
        return 0;
    }
    try {
        return render->context->areCoarsePassesComplete() ? 1 : 0;
    } catch (...) {
        return 0;
    }
}

int nmr_render_is_frame_complete(const NmrRender* render)
{
    if (render == nullptr || !render->context) {
        return 0;
    }
    try {
        return render->context->isFrameComplete() ? 1 : 0;
    } catch (...) {
        return 0;
    }
}

int nmr_render_resolution(const NmrRender* render, unsigned* width,
                          unsigned* height)
{
    if (render == nullptr || !render->context || width == nullptr
        || height == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    try {
        // The *rezed* window, not what the scene asked for:
        // `SceneVariables::res` scales the image, so asking the scene
        // gives a size the buffer does not have.
        const auto window = render->context->getRezedRegionWindow();
        *width = static_cast<unsigned>(window.width());
        *height = static_cast<unsigned>(window.height());
        return NMR_OK;
    } catch (...) {
        return NMR_FAILED;
    }
}

int nmr_render_snapshot(NmrRender* render, float* pixels, size_t capacity)
{
    if (pixels == nullptr) {
        return NMR_BAD_ARGUMENT;
    }
    return guarded(render, [&] {
        unsigned width = 0;
        unsigned height = 0;
        if (nmr_render_resolution(render, &width, &height) != NMR_OK) {
            return NMR_FAILED;
        }

        const size_t needed = size_t(width) * size_t(height) * 4;
        if (capacity < needed) {
            return NMR_BAD_ARGUMENT;
        }

        render->buffer.init(width, height);
        // `untile` true, because MoonRay renders in tile order and a
        // caller wants rows. `parallel` true costs nothing here.
        render->context->snapshotRenderBuffer(&render->buffer, true, true,
                                              true);

        const scene_rdl2::fb_util::RenderColor* source =
            render->buffer.getData();
        for (size_t i = 0; i < size_t(width) * size_t(height); ++i) {
            pixels[i * 4 + 0] = source[i].x;
            pixels[i * 4 + 1] = source[i].y;
            pixels[i * 4 + 2] = source[i].z;
            pixels[i * 4 + 3] = source[i].w;
        }
        return NMR_OK;
    });
}

} // extern "C"
