// SPDX-License-Identifier: MIT
// Original Caskrun diagnostic. Bounded capability enumeration / device initialization.
// No drawing, presentation, AnyPS5 execution or gameplay validation.
// Targeted requirement snapshots: AnyPS5 v0.1.1 and
// 8b393b6e9a8ade9188d4f74a0293e0efd9828597. See README.md for scope.
#define WIN32_LEAN_AND_MEAN
#define VK_NO_PROTOTYPES
#include <dlfcn.h>
#include <vulkan/vulkan_core.h>
#include <cstdio>
#include <cstring>
#include <vector>

static constexpr uint32_t bound = 512;
static void version(const char *label, uint32_t value) {
    std::printf("%s=%u.%u.%u\n", label, VK_API_VERSION_MAJOR(value),
                VK_API_VERSION_MINOR(value), VK_API_VERSION_PATCH(value));
}
static bool advertised(const std::vector<VkExtensionProperties> &list, const char *name) {
    for (const auto &extension : list)
        if (std::strncmp(extension.extensionName, name, VK_MAX_EXTENSION_NAME_SIZE) == 0)
            return true;
    return false;
}
static void capability(const char *name, bool value) {
    std::printf("%s=%s\n", name, value ? "yes" : "no");
}
template<class T> static T lookup(PFN_vkGetInstanceProcAddr get, VkInstance instance,
                                  const char *name) {
    return reinterpret_cast<T>(get(instance, name));
}
static int fail(const char *operation, VkResult result) {
    std::printf("%s_failed=%d\n", operation, static_cast<int>(result));
    return 1;
}

int main(int argc, char **argv) {
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    std::puts("probe=original-native-vulkan-initialization-and-requirements-v2");
    if (argc != 2) { std::puts("loader_argument_required=yes"); return 2; }
    void *dll = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!dll) {
        std::puts("load_native_vulkan_loader_failed=yes");
        return 1;
    }
    void *addressFromDll = dlsym(dll, "vkGetInstanceProcAddr");
    PFN_vkGetInstanceProcAddr get = nullptr;
    static_assert(sizeof(get) == sizeof(addressFromDll), "Native function address size");
    std::memcpy(&get, &addressFromDll, sizeof(get));
    if (!get) { std::puts("get_instance_proc_addr_missing=yes"); dlclose(dll); return 1; }
    auto enumerateVersion = lookup<PFN_vkEnumerateInstanceVersion>(get, VK_NULL_HANDLE, "vkEnumerateInstanceVersion");
    auto enumerateExtensions = lookup<PFN_vkEnumerateInstanceExtensionProperties>(get, VK_NULL_HANDLE, "vkEnumerateInstanceExtensionProperties");
    auto createInstance = lookup<PFN_vkCreateInstance>(get, VK_NULL_HANDLE, "vkCreateInstance");
    if (!enumerateExtensions || !createInstance) { std::puts("global_functions_missing=yes"); dlclose(dll); return 1; }
    uint32_t loaderVersion = VK_API_VERSION_1_0;
    if (enumerateVersion) {
        VkResult result = enumerateVersion(&loaderVersion);
        if (result != VK_SUCCESS) { dlclose(dll); return fail("enumerate_instance_version", result); }
    }
    version("loader_api", loaderVersion);
    uint32_t extensionCount = 0;
    VkResult result = enumerateExtensions(nullptr, &extensionCount, nullptr);
    if (result != VK_SUCCESS || extensionCount > bound) { dlclose(dll); return fail("instance_extension_count", result); }
    std::vector<VkExtensionProperties> instanceExtensions(extensionCount);
    if (extensionCount) {
        result = enumerateExtensions(nullptr, &extensionCount, instanceExtensions.data());
        if (result != VK_SUCCESS) { dlclose(dll); return fail("instance_extensions", result); }
        instanceExtensions.resize(extensionCount);
    }
    std::printf("instance_extension_count=%u\n", extensionCount);
    const bool surfaceExtension = advertised(instanceExtensions, "VK_KHR_surface");
    const bool win32SurfaceExtension = advertised(instanceExtensions, "VK_KHR_win32_surface");
    capability("instance.VK_KHR_surface", surfaceExtension);
    capability("instance.VK_KHR_win32_surface", win32SurfaceExtension);
    const bool portability = advertised(instanceExtensions, VK_KHR_PORTABILITY_ENUMERATION_EXTENSION_NAME);
    capability("instance.VK_KHR_portability_enumeration", portability);
    // Standard portability enumeration is enabled only when the driver advertises it.
    const char *portabilityExtension = VK_KHR_PORTABILITY_ENUMERATION_EXTENSION_NAME;
    VkApplicationInfo application{};
    application.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO;
    application.pApplicationName = "Caskrun Vulkan Capability Probe";
    application.apiVersion = loaderVersion >= VK_API_VERSION_1_1 ? VK_API_VERSION_1_1 : VK_API_VERSION_1_0;
    VkInstanceCreateInfo instanceInfo{};
    instanceInfo.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO;
    instanceInfo.pApplicationInfo = &application;
    if (portability) {
        instanceInfo.flags = VK_INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR;
        instanceInfo.enabledExtensionCount = 1;
        instanceInfo.ppEnabledExtensionNames = &portabilityExtension;
    }
    VkInstance instance = VK_NULL_HANDLE;
    result = createInstance(&instanceInfo, nullptr, &instance);
    std::printf("create_instance=%d\n", static_cast<int>(result));
    if (result != VK_SUCCESS) { dlclose(dll); return 1; }
    auto destroyInstance = lookup<PFN_vkDestroyInstance>(get, instance, "vkDestroyInstance");
    auto enumerateDevices = lookup<PFN_vkEnumeratePhysicalDevices>(get, instance, "vkEnumeratePhysicalDevices");
    auto getProperties = lookup<PFN_vkGetPhysicalDeviceProperties>(get, instance, "vkGetPhysicalDeviceProperties");
    auto getProperties2 = lookup<PFN_vkGetPhysicalDeviceProperties2>(get, instance, "vkGetPhysicalDeviceProperties2");
    auto getFeatures2 = lookup<PFN_vkGetPhysicalDeviceFeatures2>(get, instance, "vkGetPhysicalDeviceFeatures2");
    auto getFeatures = lookup<PFN_vkGetPhysicalDeviceFeatures>(get, instance, "vkGetPhysicalDeviceFeatures");
    auto enumerateDeviceExtensions = lookup<PFN_vkEnumerateDeviceExtensionProperties>(get, instance, "vkEnumerateDeviceExtensionProperties");
    auto getQueues = lookup<PFN_vkGetPhysicalDeviceQueueFamilyProperties>(get, instance, "vkGetPhysicalDeviceQueueFamilyProperties");
    auto createDevice = lookup<PFN_vkCreateDevice>(get, instance, "vkCreateDevice");
    auto getDeviceProc = lookup<PFN_vkGetDeviceProcAddr>(get, instance, "vkGetDeviceProcAddr");
    if (!destroyInstance || !enumerateDevices || !getProperties || !getFeatures ||
        !enumerateDeviceExtensions || !getQueues || !createDevice || !getDeviceProc) {
        std::puts("instance_functions_missing=yes");
        if (destroyInstance) destroyInstance(instance, nullptr);
        dlclose(dll); return 1;
    }
    uint32_t deviceCount = 0;
    result = enumerateDevices(instance, &deviceCount, nullptr);
    if (result != VK_SUCCESS || deviceCount == 0 || deviceCount > 16) {
        std::printf("physical_device_count=%u\n", deviceCount);
        destroyInstance(instance, nullptr); dlclose(dll); return fail("enumerate_devices", result);
    }
    std::vector<VkPhysicalDevice> devices(deviceCount);
    result = enumerateDevices(instance, &deviceCount, devices.data());
    if (result != VK_SUCCESS) { destroyInstance(instance, nullptr); dlclose(dll); return fail("enumerate_devices", result); }
    std::printf("physical_device_count=%u\n", deviceCount);
    bool initializedAny = false;
    for (uint32_t index = 0; index < deviceCount; ++index) {
        VkPhysicalDevice physical = devices[index];
        std::printf("device_index=%u\n", index);
        VkPhysicalDeviceProperties properties{};
        getProperties(physical, &properties);
        version("device_api", properties.apiVersion);
        capability("required.api_at_least_1_1", properties.apiVersion >= VK_API_VERSION_1_1);
        uint32_t count = 0;
        result = enumerateDeviceExtensions(physical, nullptr, &count, nullptr);
        if (result != VK_SUCCESS || count > bound) { fail("device_extension_count", result); continue; }
        std::vector<VkExtensionProperties> extensions(count);
        if (count) {
            result = enumerateDeviceExtensions(physical, nullptr, &count, extensions.data());
            if (result != VK_SUCCESS) { fail("device_extensions", result); continue; }
            extensions.resize(count);
        }
        std::printf("device_extension_count=%u\n", count);
        const bool storage = advertised(extensions, VK_KHR_8BIT_STORAGE_EXTENSION_NAME);
        const bool address = advertised(extensions, VK_KHR_BUFFER_DEVICE_ADDRESS_EXTENSION_NAME);
        const bool controls = advertised(extensions, VK_KHR_SHADER_FLOAT_CONTROLS_EXTENSION_NAME);
        const bool robustness = advertised(extensions, VK_EXT_ROBUSTNESS_2_EXTENSION_NAME);
        const bool subset = advertised(extensions, "VK_KHR_portability_subset");
        capability("extension.VK_KHR_8bit_storage", storage);
        capability("extension.VK_KHR_buffer_device_address", address);
        capability("extension.VK_KHR_shader_float_controls", controls);
        capability("extension.VK_EXT_robustness2", robustness);
        capability("extension.VK_KHR_portability_subset", subset);
        const bool swapchainExtension = advertised(extensions, "VK_KHR_swapchain");
        capability("extension.VK_KHR_swapchain", swapchainExtension);
        VkPhysicalDeviceFeatures core{};
        getFeatures(physical, &core);
        capability("feature.shaderInt64", core.shaderInt64);
        capability("feature.vertexPipelineStoresAndAtomics", core.vertexPipelineStoresAndAtomics);
        capability("feature.fragmentStoresAndAtomics", core.fragmentStoresAndAtomics);
        capability("feature.samplerAnisotropy", core.samplerAnisotropy);
        capability("feature.textureCompressionBC", core.textureCompressionBC);
        VkPhysicalDevice8BitStorageFeatures storageFeatures{};
        storageFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_8BIT_STORAGE_FEATURES;
        VkPhysicalDeviceBufferDeviceAddressFeatures addressFeatures{};
        addressFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_BUFFER_DEVICE_ADDRESS_FEATURES;
        VkPhysicalDeviceRobustness2FeaturesEXT robustnessFeatures{};
        robustnessFeatures.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ROBUSTNESS_2_FEATURES_EXT;
        VkPhysicalDeviceFeatures2 features{};
        features.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2;
        void **tail = &features.pNext;
        if (storage || properties.apiVersion >= VK_API_VERSION_1_2) {
            *tail = &storageFeatures; tail = &storageFeatures.pNext;
        }
        if (address || properties.apiVersion >= VK_API_VERSION_1_2) {
            *tail = &addressFeatures; tail = &addressFeatures.pNext;
        }
        if (robustness) { *tail = &robustnessFeatures; }
        if (getFeatures2) getFeatures2(physical, &features);
        capability("query.features2_available", getFeatures2 != nullptr);
        capability("feature.storageBuffer8BitAccess", storageFeatures.storageBuffer8BitAccess);
        capability("feature.bufferDeviceAddress", addressFeatures.bufferDeviceAddress);
        capability("main_feature.nullDescriptor", robustnessFeatures.nullDescriptor);
        VkPhysicalDeviceFloatControlsProperties floats{};
        floats.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FLOAT_CONTROLS_PROPERTIES;
        VkPhysicalDeviceProperties2 properties2{};
        properties2.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2;
        if (controls || properties.apiVersion >= VK_API_VERSION_1_2) properties2.pNext = &floats;
        if (getProperties2) getProperties2(physical, &properties2);
        capability("query.properties2_available", getProperties2 != nullptr);
        capability("property.shaderSignedZeroInfNanPreserveFloat32", floats.shaderSignedZeroInfNanPreserveFloat32);
        const bool releaseDeviceFeatures = properties.apiVersion >= VK_API_VERSION_1_1 && storage && address && controls &&
            core.shaderInt64 && core.vertexPipelineStoresAndAtomics && core.fragmentStoresAndAtomics &&
            core.samplerAnisotropy && core.textureCompressionBC && storageFeatures.storageBuffer8BitAccess &&
            addressFeatures.bufferDeviceAddress && floats.shaderSignedZeroInfNanPreserveFloat32;
        // Native macOS has no Win32 surface. Compare GPU device features with
        // Wine separately from platform-specific window-system integration.
        const bool nativePresentationExtensions = surfaceExtension && swapchainExtension;
        capability("v0_1_1_targeted_device_features_present", releaseDeviceFeatures);
        capability("native_surface_swapchain_extensions_advertised", nativePresentationExtensions);
        capability("main_8b393b6_targeted_device_features_present",
                   releaseDeviceFeatures && robustness && robustnessFeatures.nullDescriptor);
        getQueues(physical, &count, nullptr);
        if (count == 0 || count > bound) { std::puts("queue_family_count_out_of_bounds=yes"); continue; }
        std::vector<VkQueueFamilyProperties> queues(count);
        getQueues(physical, &count, queues.data());
        uint32_t graphicsQueue = count;
        for (uint32_t queue = 0; queue < count; ++queue)
            if (queues[queue].queueCount && (queues[queue].queueFlags & VK_QUEUE_GRAPHICS_BIT)) { graphicsQueue = queue; break; }
        if (graphicsQueue == count) { std::puts("graphics_queue_missing=yes"); continue; }
        std::printf("graphics_queue_family=%u\n", graphicsQueue);
        float priority = 1.0f;
        VkDeviceQueueCreateInfo queueInfo{};
        queueInfo.sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO;
        queueInfo.queueFamilyIndex = graphicsQueue;
        queueInfo.queueCount = 1;
        queueInfo.pQueuePriorities = &priority;
        VkDeviceCreateInfo deviceInfo{};
        deviceInfo.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO;
        deviceInfo.queueCreateInfoCount = 1;
        deviceInfo.pQueueCreateInfos = &queueInfo;
        // This baseline creates a device without enabling optional app features.
        // A portability driver requires its advertised subset extension enabled.
        const char *subsetExtension = "VK_KHR_portability_subset";
        if (subset) { deviceInfo.enabledExtensionCount = 1; deviceInfo.ppEnabledExtensionNames = &subsetExtension; }
        VkDevice device = VK_NULL_HANDLE;
        result = createDevice(physical, &deviceInfo, nullptr, &device);
        std::printf("minimal_create_device=%d\n", static_cast<int>(result));
        if (result != VK_SUCCESS) continue;
        auto destroyDevice = reinterpret_cast<PFN_vkDestroyDevice>(getDeviceProc(device, "vkDestroyDevice"));
        auto getQueue = reinterpret_cast<PFN_vkGetDeviceQueue>(getDeviceProc(device, "vkGetDeviceQueue"));
        auto waitIdle = reinterpret_cast<PFN_vkDeviceWaitIdle>(getDeviceProc(device, "vkDeviceWaitIdle"));
        VkQueue queue = VK_NULL_HANDLE;
        if (getQueue) getQueue(device, graphicsQueue, 0, &queue);
        capability("minimal_graphics_queue_created", queue != VK_NULL_HANDLE);
        VkResult idleResult = waitIdle ? waitIdle(device) : VK_ERROR_INITIALIZATION_FAILED;
        std::printf("minimal_device_wait_idle=%d\n", static_cast<int>(idleResult));
        initializedAny = initializedAny || (queue != VK_NULL_HANDLE && idleResult == VK_SUCCESS);
        if (destroyDevice) destroyDevice(device, nullptr);
    }
    destroyInstance(instance, nullptr);
    dlclose(dll);
    std::puts("scope=baseline-only-no-drawing-presentation-or-gameplay");
    return initializedAny ? 0 : 1;
}
