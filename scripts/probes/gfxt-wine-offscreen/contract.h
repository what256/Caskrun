/* Original Caskrun experimental fixture protocol. No object pointers cross PE. */
#pragma once
#include <stdint.h>
struct caskrun_offscreen_request {
    uint32_t version;
    uint32_t size;
    int32_t result;
    uint32_t native_pointers_returned;
};
