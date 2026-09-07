// An *authoring* twin of MoonRay's `RdlInstancerGeometry`.
//
// See `RdlMeshGeometry.cc` for why these exist and what they are not.
//
// The declarations, with no implementation. Its point is that
// `oracle verify` — and anything else that wants to read a mesh scene
// back through rdl2 — can do so on a host with `scene_rdl2` alone,
// which builds in about fifteen minutes from stock packages. MoonRay
// itself is a fifty-minute build with five packaging problems in the
// way, and needing it to check that a mesh was written correctly is
// the difference between a fast loop and a slow one.
//
// **The attributes are MoonRay's own file, included verbatim.** Not a
// copy: `attributes.cc` is compiled straight out of a `moonray` source
// checkout, so this twin cannot drift from what the renderer declares.
// A copy would go stale in exactly the way that renders a scene
// plausibly and wrongly — an attribute renamed upstream, still spelled
// the old way here, and silently ignored by the real DSO.
//
// It renders nothing. `createProcedural` returns null and
// `destroyProcedural` does nothing, which is all a scene needs to be
// *built*, *written* and *read back*. Loading this into a renderer
// would give an empty image, so do not: `$RDL2_DSO_PATH` should point
// at MoonRay's own `rdl2dso` for that.

#include <scene_rdl2/scene/rdl2/rdl2.h>

// MoonRay's, from its own source tree. See the note above.
#include "attributes.cc"

RDL2_DSO_CLASS_BEGIN(RdlInstancerGeometry, scene_rdl2::rdl2::Geometry)

public:
    RDL2_DSO_DEFAULT_CTOR(RdlInstancerGeometry)

    // `Geometry` declares these pure virtual. `moonray::geom::Procedural`
    // is only forward-declared in `Geometry.h`, so returning null needs
    // none of MoonRay's headers — which is the whole reason this twin
    // can be built without it.
    moonray::geom::Procedural* createProcedural() const override
    {
        return nullptr;
    }

    // Never called with anything to destroy, since nothing is created.
    void destroyProcedural() const override {}

RDL2_DSO_CLASS_END(RdlInstancerGeometry)
