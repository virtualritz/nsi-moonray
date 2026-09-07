// An *authoring* twin of MoonRay's `EnvLight`.
//
// See `RdlMeshGeometry.cc` for why these exist and what they are not.

#include <scene_rdl2/common/math/Math.h>
#include <scene_rdl2/scene/rdl2/rdl2.h>

// MoonRay's own, compiled out of its source tree rather than copied.
#include "attributes.cc"

// As MoonRay's own DSO does: the macro names the base unqualified.
using namespace scene_rdl2;

RDL2_DSO_CLASS_BEGIN(EnvLight, rdl2::Light)

public:
    RDL2_DSO_DEFAULT_CTOR(EnvLight)

RDL2_DSO_CLASS_END(EnvLight)
