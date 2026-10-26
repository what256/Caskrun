/*
 * GFXT root and registry declarations adapted from the MIT-licensed
 * utmapp/d3dmetal-native, revision 422df8736de0f371d5ecee71e04b3ed19c4a2183.
 * Copyright 2026 Turing Software LLC. See UTM-MIT-LICENSE.
 * This mock-only prototype uses these types as published, without hooks.
 */
#ifndef CASKRUN_GFXT_TYPED_INTERFACE_H
#define CASKRUN_GFXT_TYPED_INTERFACE_H
#include <cstdint>
#include <string>
#include <vector>

struct GFXTInterfaceVersion { uint32_t version; uint32_t padding; };
class GFXTMonitorInterface;
class GFXTEventInterface;
class GFXTSwapchainInterface;
class GFXTAdapterInterface;
class GFXTPathInterface;
class GFXTAllocationInterface;
class GFXTLibraryInterface;

class GFXTRegistryInterface {
public:
    enum RegistryMainKey : uint32_t { HKEY_CLASSES_ROOT_, HKEY_CURRENT_USER_, HKEY_LOCAL_MACHINE_, HKEY_USERS_ };
    virtual GFXTInterfaceVersion Version() const = 0;
    virtual ~GFXTRegistryInterface() = default;
    virtual void* OpenKey(RegistryMainKey, const char*) = 0;
    virtual void* CreateKey(RegistryMainKey, const char*, bool, bool*) = 0;
    virtual void CloseKey(void*) = 0;
    virtual void SetValue(void*, const char*, uint32_t) = 0;
    virtual void SetValue(void*, const char*, uint64_t) = 0;
    virtual void SetValue(void*, const char*, const std::string&) = 0;
    virtual void SetValue(void*, const char*, const std::vector<uint8_t>&) = 0;
    virtual bool GetValue(void*, const char*, uint32_t&) = 0;
    virtual bool GetValue(void*, const char*, uint64_t&) = 0;
    virtual bool GetValue(void*, const char*, std::string&) = 0;
    virtual bool GetValue(void*, const char*, std::vector<uint8_t>&) = 0;
    virtual bool DeleteValue(void*, const char*, const char*) = 0;
};

class GFXTOSInterface {
public:
    virtual GFXTInterfaceVersion Version() const = 0;
    virtual ~GFXTOSInterface() = default;
    virtual GFXTMonitorInterface* CreateMonitorInterface(const GFXTInterfaceVersion&) = 0;
    virtual GFXTRegistryInterface* CreateRegistryInterface(const GFXTInterfaceVersion&) = 0;
    virtual GFXTEventInterface* CreateEventInterface(const GFXTInterfaceVersion&) = 0;
    virtual GFXTSwapchainInterface* CreateSwapchainInterface(const GFXTInterfaceVersion&, void*) = 0;
    virtual GFXTAdapterInterface* CreateAdapterInterface(const GFXTInterfaceVersion&) = 0;
    virtual GFXTPathInterface* CreatePathInterface(const GFXTInterfaceVersion&) = 0;
    virtual GFXTAllocationInterface* CreateAllocationInterface(const GFXTInterfaceVersion&) = 0;
    virtual GFXTLibraryInterface* CreateLibraryInterface(const GFXTInterfaceVersion&) = 0;
};
#endif
