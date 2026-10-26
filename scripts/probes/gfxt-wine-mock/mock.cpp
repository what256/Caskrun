/* Original Caskrun mock; no Apple component is loaded or impersonated. MIT. */
#include "typed_gfxt.h"
#include <cstdint>
#include <new>

namespace {
GFXTRegistryInterface* registry;
unsigned created, released;
struct MockFactory { uint32_t marker; };
}

extern "C" bool caskrun_mock_initialize(GFXTOSInterface* host)
{
    if (!host || registry || host->Version().version != 5) return false;
    const GFXTInterfaceVersion requested{2, 0};
    registry = host->CreateRegistryInterface(requested);
    created = released = 0;
    return registry && registry->Version().version == 2;
}

/* A separate translation unit forces a genuine SysV-to-MSABI call. The
 * opaque object is our own mock allocation; it never crosses into PE code. */
extern "C" int32_t __attribute__((ms_abi)) caskrun_mock_factory(
    void* actual_wine_registry_handle, uint32_t expected, void** out)
{
    if (!registry || !out) return (int32_t)0x80004005u;
    *out = nullptr;
    uint32_t value = 0;
    if (!registry->GetValue(actual_wine_registry_handle, "probe", value) || value != expected)
        return (int32_t)0x80004005u;
    auto* factory = new (std::nothrow) MockFactory{0x43474654u};
    if (!factory) return (int32_t)0x8007000eu;
    *out = factory;
    ++created;
    return 0;
}

extern "C" bool caskrun_mock_release(void* object)
{
    auto* factory = static_cast<MockFactory*>(object);
    if (!factory || factory->marker != 0x43474654u || released >= created) return false;
    factory->marker = 0;
    delete factory;
    ++released;
    return true;
}

extern "C" bool caskrun_mock_shutdown()
{
    bool ok = registry && created == 1 && released == 1;
    registry = nullptr;
    return ok;
}
