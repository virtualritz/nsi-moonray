/// An OSL volume shader, as a MoonRay `VolumeShader`.
///
/// ɴsɪ binds one through the `attributes` node's `volumeshader`
/// connection, the way it binds a surface through `surfaceshader`. The
/// group that arrives here is built by the same code, with OSL's
/// `"volume"` usage.
///
/// # Four questions, one execution
///
/// `scene_rdl2::rdl2::VolumeShader` is four pure virtuals --
/// `extinct`, `albedo`, `emission`, `anisotropy` -- each asked on its
/// own and two of them carrying a `rayVolumeDepth` the others do not.
/// OSL runs once and produces one closure tree. So the tree is walked
/// once per shading point and the four answers are cached together;
/// the next of the four to be asked about the same point reads the
/// cache rather than shading again.
///
/// The cache is **thread-local and one deep**. MoonRay asks the four
/// in a run for a given point, so one entry catches every repeat; a
/// larger cache would need a key that is stable across calls, and a
/// `State` does not offer one. Getting the key wrong would return
/// another point's density, which is a plausible puff of the wrong
/// shape -- so the key is the state's address *and* the density it was
/// asked with, and a miss simply shades again.
///
/// # `getProperties` is answered before anything shades
///
/// It is a bitmask MoonRay uses to decide what to sample at all, and
/// it is read at render prep -- before a single closure exists. OSL
/// cannot answer it without running, so the answer is the conservative
/// superset: extinctive, scattering and emissive, none of them
/// homogeneous. That is correct and slow. Claiming less would be fast
/// and wrong in the way this backend keeps meeting: MoonRay would stop
/// sampling emission, and the volume would render without it, and
/// nothing would say so.

#define NSI_MOONRAY_OSL_ROOT rdl2::VolumeShader
#include "attributes.cc"
#include "shading_system.h"

#include <moonray/rendering/shading/State.h>
#include <moonray/rendering/shading/Xform.h>

#include <scene_rdl2/scene/rdl2/rdl2.h>

#include <cmath>
#include <cstdlib>
#include <memory>
#include <string>

using namespace moonray::shading;
using namespace nsi_moonray;
using scene_rdl2::math::Color;

namespace {

/// What one walk of a volume closure tree accumulates.
struct Sample {
    Color extinction { 0.0f, 0.0f, 0.0f };
    Color albedo { 0.0f, 0.0f, 0.0f };
    Color emission { 0.0f, 0.0f, 0.0f };
    /// Weighted by extinction, so a thin medium does not pull the
    /// phase function of a thick one around.
    float anisotropy = 0.0f;
    float weight = 0.0f;
};

Color
to_color(const OSL::Color3& c)
{
    return Color(c.x, c.y, c.z);
}

/// `medium_vdf` describes a medium by how far light gets through it,
/// so its extinction is implied rather than given.
///
/// Beer's law inverted, per channel: a transmission colour `t` reached
/// at depth `d` means `sigma = -log(t) / d`. A channel at or below
/// zero is opaque, which is an infinite extinction, and is clamped to
/// something large rather than propagated as an infinity that MoonRay
/// would carry into a ray distance.
Color
extinction_of(const Color& transmission, float depth)
{
    const float safe_depth = depth > 1e-6f ? depth : 1e-6f;
    float channel[3];
    const float value[3] = { transmission.r, transmission.g,
                             transmission.b };
    for (int i = 0; i < 3; ++i) {
        channel[i] = value[i] > 1e-6f
                         ? -std::log(value[i]) / safe_depth
                         : 1e6f;
    }
    return Color(channel[0], channel[1], channel[2]);
}

void
walk_volume(Sample& sample, const OSL::ClosureColor* closure,
            const Color& weight)
{
    if (closure == nullptr) {
        return;
    }

    switch (closure->id) {
    case OSL::ClosureColor::MUL: {
        const auto* mul = closure->as_mul();
        walk_volume(sample, mul->closure,
                    weight * to_color(mul->weight));
        return;
    }
    case OSL::ClosureColor::ADD: {
        const auto* add = closure->as_add();
        walk_volume(sample, add->closureA, weight);
        walk_volume(sample, add->closureB, weight);
        return;
    }
    default:
        break;
    }

    const auto* component = closure->as_comp();
    const Color total = weight * to_color(component->w);

    switch (component->id) {
    case CLOSURE_ANISOTROPIC_VDF: {
        const auto* params = component->as<AnisotropicVdfParams>();
        const Color extinction = total * to_color(params->extinction);
        const float magnitude =
            extinction.r + extinction.g + extinction.b;
        sample.extinction = sample.extinction + extinction;
        sample.albedo = sample.albedo + total * to_color(params->albedo);
        sample.anisotropy += params->anisotropy * magnitude;
        sample.weight += magnitude;
        return;
    }
    case CLOSURE_MEDIUM_VDF: {
        const auto* params = component->as<MediumVdfParams>();
        const Color extinction =
            total * extinction_of(to_color(params->transmission_color),
                                  params->transmission_depth);
        const float magnitude =
            extinction.r + extinction.g + extinction.b;
        sample.extinction = sample.extinction + extinction;
        sample.albedo = sample.albedo + total * to_color(params->albedo);
        sample.anisotropy += params->anisotropy * magnitude;
        sample.weight += magnitude;
        return;
    }
    case CLOSURE_EMISSION:
        sample.emission = sample.emission + total;
        return;
    default:
        // A surface closure in a volume shader. Ignored rather than
        // refused: the volume still renders, and refusing a scene is
        // not something this backend does.
        return;
    }
}

} // namespace

RDL2_DSO_CLASS_BEGIN(OslVolume, scene_rdl2::rdl2::VolumeShader)

public:
    OslVolume(const scene_rdl2::rdl2::SceneClass& sceneClass,
              const std::string& name);

    void update() override;

    unsigned getProperties() const override
    {
        // The conservative superset. See the note at the top.
        return IS_EXTINCTIVE | IS_SCATTERING | IS_EMISSIVE;
    }

    Color extinct(moonray::shading::TLState* tls, const State& state,
                  const Color& density, float) const override
    {
        return shade(tls, state, density).extinction * density;
    }

    Color albedo(moonray::shading::TLState* tls, const State& state,
                 const Color& density, float) const override
    {
        return shade(tls, state, density).albedo;
    }

    Color emission(moonray::shading::TLState* tls, const State& state,
                   const Color& density) const override
    {
        return shade(tls, state, density).emission * density;
    }

    float anisotropy(moonray::shading::TLState* tls,
                     const State& state) const override
    {
        const Sample& sample = shade(tls, state, Color(1.0f, 1.0f, 1.0f));
        return sample.weight > 0.0f ? sample.anisotropy / sample.weight
                                    : 0.0f;
    }

    /// No map is bound to extinction: it comes out of the shader.
    bool hasExtinctionMapBinding() const override { return false; }

    /// Nothing here is baked, so nothing here needs rebaking.
    bool updateBakeRequired() const override { return false; }

private:
    const Sample& shade(moonray::shading::TLState* tls,
                        const State& state, const Color& density) const;

    OSL::ShaderGroupRef mGroup;
    std::unique_ptr<Xform> mXform;
    Attributes mAttributes;

RDL2_DSO_CLASS_END(OslVolume)

OslVolume::OslVolume(const scene_rdl2::rdl2::SceneClass& sceneClass,
                     const std::string& name)
    : Parent(sceneClass, name)
{
}

void
OslVolume::update()
{
    mGroup.reset();
    mXform = std::make_unique<Xform>(this);

    const std::string& spec = get(attrGroupSpec);
    if (spec.empty()) {
        error("no OSL group specification; this volume is not shaded");
        return;
    }

    std::string search = get(attrSearchPath);
    if (search.empty()) {
        if (const char* fromEnvironment = std::getenv("OSL_SHADER_PATH")) {
            search = fromEnvironment;
        }
    }
    add_search_path(search);

    OSL::ShadingSystem& shading = shading_system();
    const std::string& groupName = get(attrGroupName);

    mGroup = shading.ShaderGroupBegin(
        groupName.empty() ? getName() : groupName, "volume", spec);
    if (!mGroup) {
        error("the OSL group specification did not parse; this volume "
              "is not shaded");
        return;
    }
    shading.ShaderGroupEnd(*mGroup);

    shading.optimize_group(mGroup.get(), nullptr, true);
    mAttributes = Attributes::of(mGroup);
    mOptionalAttributes = mAttributes.keys();
}

const Sample&
OslVolume::shade(moonray::shading::TLState* /*tls*/, const State& state,
                 const Color& density) const
{
    // One entry, per thread. The four virtuals are asked in a run for
    // one point, so this catches every repeat within that run.
    thread_local Sample cached;
    thread_local const State* cachedState = nullptr;
    thread_local Color cachedDensity { -1.0f, -1.0f, -1.0f };

    if (cachedState == &state && cachedDensity.r == density.r
        && cachedDensity.g == density.g
        && cachedDensity.b == density.b) {
        return cached;
    }

    cached = Sample {};
    cachedState = &state;
    cachedDensity = density;

    if (!mGroup) {
        return cached;
    }

    OSL::ShadingSystem& shading = shading_system();
    thread_local OSL::PerThreadInfo* threadInfo =
        shading.create_thread_info();
    thread_local OSL::ShadingContext* context =
        shading.get_context(threadInfo);

    const scene_rdl2::math::Vec3f& position = state.getP();
    const scene_rdl2::math::Vec3f& normal = state.getN();

    OSL::ShaderGlobals globals = {};
    globals.P = OSL::Vec3(position.x, position.y, position.z);
    globals.N = OSL::Vec3(normal.x, normal.y, normal.z);
    globals.Ng = globals.N;
    globals.surfacearea = 1.0f;
    globals.raytype = 1;

    const ShadingPoint point { mXform.get(), &state, &mAttributes };
    globals.renderstate = const_cast<ShadingPoint*>(&point);
    globals.object2common =
        reinterpret_cast<OSL::TransformationPtr>(&point);
    globals.shader2common =
        reinterpret_cast<OSL::TransformationPtr>(&point);

    shading.execute(context, *mGroup, globals);

    walk_volume(cached,
                reinterpret_cast<const OSL::ClosureColor*>(globals.Ci),
                Color(1.0f, 1.0f, 1.0f));
    return cached;
}
