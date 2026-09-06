// Does the shim actually drive rdl2? Built against the real library,
// using rdl2's own test DSOs for a class with attributes of every type.
#include "nsi_moonray_shim.h"
#include <stdio.h>
#include <string.h>

static int failures = 0;
static void check(const char* what, int code) {
    if (code != NMR_OK) { printf("FAIL %s -> %d\n", what, code); failures++; }
}

int main(int argc, char** argv) {
    if (argc < 3) { printf("usage: smoke <dso_path> <out.rdla>\n"); return 2; }

    NmrContext* ctx = nmr_context_new(argv[1]);
    if (!ctx) { printf("FAIL context\n"); return 1; }

    NmrObject* geo = nmr_object(ctx, "ExtensiveObject", "/geo");
    if (!geo) { printf("FAIL create: %s\n", nmr_context_error(ctx)); return 1; }

    check("begin", nmr_begin_update(geo));
    check("bool",   nmr_set_bool(geo, "bool", 1, NMR_WHOLE_SHUTTER));
    check("int",    nmr_set_int(geo, "int", 42, NMR_WHOLE_SHUTTER));
    check("float",  nmr_set_float(geo, "float", 0.5f, NMR_WHOLE_SHUTTER));
    check("string", nmr_set_string(geo, "string", "hello", NMR_WHOLE_SHUTTER));
    const double m[16] = {1,0,0,0, 0,1,0,0, 0,0,1,0, 7,0,0,1};
    check("mat4d",  nmr_set_mat4d(geo, "mat4d", m, NMR_WHOLE_SHUTTER));
    const int ints[3] = {1,2,3};
    check("int_vec", nmr_set_int_vector(geo, "int_vector", ints, 3));
    const double mats[32] = {1,0,0,0, 0,1,0,0, 0,0,1,0, 10,0,0,1,
                             1,0,0,0, 0,1,0,0, 0,0,1,0, 20,0,0,1};
    check("mat4d_vec", nmr_set_mat4d_vector(geo, "mat4d_vector", mats, 2));

    // Motion blur: two timesteps on one attribute.
    check("blur0", nmr_set_float(geo, "float", 1.0f, NMR_TIMESTEP_BEGIN));
    check("blur1", nmr_set_float(geo, "float", 2.0f, NMR_TIMESTEP_END));
    check("end",   nmr_end_update(geo));

    // The two failure modes must be distinguishable, not both "failed".
    int missing = nmr_set_int(geo, "no_such_attribute_at_all", 1,
                              NMR_WHOLE_SHUTTER);
    printf("%s missing-attribute -> %d (want %d)\n",
           missing == NMR_NO_SUCH_ATTRIBUTE ? "ok  " : "FAIL",
           missing, NMR_NO_SUCH_ATTRIBUTE);
    if (missing != NMR_NO_SUCH_ATTRIBUTE) failures++;

    int wrong = nmr_set_int(geo, "string", 1, NMR_WHOLE_SHUTTER);
    printf("%s wrong-type        -> %d (want %d)\n",
           wrong == NMR_TYPE_MISMATCH ? "ok  " : "FAIL",
           wrong, NMR_TYPE_MISMATCH);
    if (wrong != NMR_TYPE_MISMATCH) failures++;

    check("write", nmr_context_write_ascii(ctx, argv[2]));
    nmr_context_free(ctx);

    printf("%s\n", failures ? "SMOKE FAILED" : "SMOKE OK");
    return failures ? 1 : 0;
}
