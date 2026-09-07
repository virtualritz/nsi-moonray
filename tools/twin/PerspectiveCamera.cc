// An *authoring* twin of MoonRay's `PerspectiveCamera`.
//
// See `RdlMeshGeometry.cc` for why these exist and what they are not.

#include <scene_rdl2/common/math/Math.h>
#include <scene_rdl2/scene/rdl2/rdl2.h>

// MoonRay's own, compiled out of its source tree rather than copied.
#include "attributes.cc"

// As MoonRay's own DSO does: the macro names the base unqualified.
using namespace scene_rdl2;

RDL2_DSO_CLASS_BEGIN(PerspectiveCamera, rdl2::Camera)

public:
    RDL2_DSO_DEFAULT_CTOR(PerspectiveCamera)

RDL2_DSO_CLASS_END(PerspectiveCamera)
