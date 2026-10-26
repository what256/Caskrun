/* Experimental mock-only protocol. Original Caskrun code, MIT. */
#ifndef CASKRUN_GFXT_MOCK_CONTRACT_H
#define CASKRUN_GFXT_MOCK_CONTRACT_H
#include <stdint.h>

/* The only pointer-like value crossing the boundary is an actual Windows
 * registry HANDLE obtained in the disposable PE test. No native pointer or
 * fake OS object is returned to Windows code. */
struct caskrun_gfxt_probe_params {
    uint32_t version;
    uint32_t size;
    uint64_t registry_handle;
    uint32_t expected_value;
    uint32_t checks;
    uint32_t failures;
    uint32_t native_pointers_returned;
};
#if defined(__cplusplus)
static_assert(sizeof(caskrun_gfxt_probe_params) == 32, "fixed protocol");
#else
_Static_assert(sizeof(struct caskrun_gfxt_probe_params) == 32, "fixed protocol");
#endif
#endif
