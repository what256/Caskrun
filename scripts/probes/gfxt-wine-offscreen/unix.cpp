/* Original Caskrun fixture-owned Wine Unix table, not an Apple forwarder. */
#include "contract.h"
#include <cstdint>
extern "C" int caskrun_owned_offscreen_run();
static int32_t run(void* opaque) {
    auto* request = static_cast<caskrun_offscreen_request*>(opaque);
    if (!request || request->version != 1 || request->size != sizeof(*request))
        return static_cast<int32_t>(0xc000000du);
    request->native_pointers_returned = 0;
    request->result = caskrun_owned_offscreen_run();
    return 0;
}
extern "C" __attribute__((visibility("default"))) int32_t (*const __wine_unix_call_funcs[])(void*) = {run};
