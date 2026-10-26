/* Original Caskrun mock-only Wine Unix transport. MIT. */
#include "contract.h"
#include "typed_gfxt.h"
#include <dlfcn.h>
#include <cstdint>
#include <cstring>

extern "C" bool caskrun_mock_initialize(GFXTOSInterface*);
extern "C" int32_t __attribute__((ms_abi)) caskrun_mock_factory(void*, uint32_t, void**);
extern "C" bool caskrun_mock_release(void*);
extern "C" bool caskrun_mock_shutdown();

/* Wine/Windows public NT registry argument layouts on x64. */
struct NtUnicodeString { uint16_t length, maximum_length; const char16_t* buffer; };
struct NtPartialValue { uint32_t title_index, type, length, value; };
using NtQueryValue = int32_t (*)(void*, const NtUnicodeString*, uint32_t, void*, uint32_t, uint32_t*);
static_assert(sizeof(NtUnicodeString) == 16 && sizeof(NtPartialValue) == 16, "NT layouts");

class WineRegistry final : public GFXTRegistryInterface {
    void* const borrowed;
    NtQueryValue const query;
public:
    unsigned unsupported = 0;
    WineRegistry(void* handle, NtQueryValue fn) : borrowed(handle), query(fn) {}
    GFXTInterfaceVersion Version() const override { return {2, 0}; }
    void* OpenKey(RegistryMainKey, const char*) override { ++unsupported; return nullptr; }
    void* CreateKey(RegistryMainKey, const char*, bool, bool*) override { ++unsupported; return nullptr; }
    void CloseKey(void*) override { ++unsupported; }
    void SetValue(void*, const char*, uint32_t) override { ++unsupported; }
    void SetValue(void*, const char*, uint64_t) override { ++unsupported; }
    void SetValue(void*, const char*, const std::string&) override { ++unsupported; }
    void SetValue(void*, const char*, const std::vector<uint8_t>&) override { ++unsupported; }
    bool GetValue(void* key, const char* name, uint32_t& value) override {
        if (key != borrowed || !query || !name || std::strcmp(name, "probe")) return false;
        static const char16_t text[] = u"probe";
        const NtUnicodeString field{10, 12, text};
        NtPartialValue output{};
        uint32_t returned = 0;
        const int32_t status = query(key, &field, 2, &output, sizeof(output), &returned);
        if (status || output.type != 4 || output.length != sizeof(value) || returned < sizeof(output)) return false;
        value = output.value;
        return true;
    }
    bool GetValue(void*, const char*, uint64_t&) override { ++unsupported; return false; }
    bool GetValue(void*, const char*, std::string&) override { ++unsupported; return false; }
    bool GetValue(void*, const char*, std::vector<uint8_t>&) override { ++unsupported; return false; }
    bool DeleteValue(void*, const char*, const char*) override { ++unsupported; return false; }
};

class MockOnlyHost final : public GFXTOSInterface {
    WineRegistry& registry;
public:
    unsigned unsupported = 0;
    explicit MockOnlyHost(WineRegistry& value) : registry(value) {}
    GFXTInterfaceVersion Version() const override { return {5, 0}; }
    GFXTMonitorInterface* CreateMonitorInterface(const GFXTInterfaceVersion&) override { ++unsupported; return nullptr; }
    GFXTRegistryInterface* CreateRegistryInterface(const GFXTInterfaceVersion& version) override {
        if (version.version != 2) { ++unsupported; return nullptr; }
        return &registry;
    }
    GFXTEventInterface* CreateEventInterface(const GFXTInterfaceVersion&) override { ++unsupported; return nullptr; }
    GFXTSwapchainInterface* CreateSwapchainInterface(const GFXTInterfaceVersion&, void*) override { ++unsupported; return nullptr; }
    GFXTAdapterInterface* CreateAdapterInterface(const GFXTInterfaceVersion&) override { ++unsupported; return nullptr; }
    GFXTPathInterface* CreatePathInterface(const GFXTInterfaceVersion&) override { ++unsupported; return nullptr; }
    GFXTAllocationInterface* CreateAllocationInterface(const GFXTInterfaceVersion&) override { ++unsupported; return nullptr; }
    GFXTLibraryInterface* CreateLibraryInterface(const GFXTInterfaceVersion&) override { ++unsupported; return nullptr; }
};

static int32_t probe(void* opaque)
{
    auto* request = static_cast<caskrun_gfxt_probe_params*>(opaque);
    if (!request || request->version != 1 || request->size != sizeof(*request) || !request->registry_handle)
        return (int32_t)0xc000000du;
    auto query = reinterpret_cast<NtQueryValue>(dlsym(RTLD_DEFAULT, "NtQueryValueKey"));
    WineRegistry registry(reinterpret_cast<void*>(static_cast<uintptr_t>(request->registry_handle)), query);
    MockOnlyHost host(registry);
    request->checks = request->failures = request->native_pointers_returned = 0;
    auto check = [request](bool passed) { ++request->checks; if (!passed) ++request->failures; };
    check(query != nullptr);
    const bool initialized = caskrun_mock_initialize(&host);
    check(initialized);
    void* factory = nullptr;
    const int32_t first = initialized ? caskrun_mock_factory(
        reinterpret_cast<void*>(static_cast<uintptr_t>(request->registry_handle)), request->expected_value, &factory) : -1;
    check(first == 0 && factory != nullptr);
    const bool released = factory && caskrun_mock_release(factory);
    check(released);
    factory = nullptr;
    const int32_t mismatch = initialized ? caskrun_mock_factory(
        reinterpret_cast<void*>(static_cast<uintptr_t>(request->registry_handle)), request->expected_value ^ 1u, &factory) : 0;
    check(mismatch != 0 && factory == nullptr);
    check(initialized && caskrun_mock_shutdown());
    check(host.unsupported == 0 && registry.unsupported == 0);
    return request->failures ? (int32_t)0xc0000001u : 0;
}

/* This is our own Wine Unix-call table, never an Apple forwarder table. */
extern "C" __attribute__((visibility("default"))) int32_t (*const __wine_unix_call_funcs[])(void*) = {probe};
