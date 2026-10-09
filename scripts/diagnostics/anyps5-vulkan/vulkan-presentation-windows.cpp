// SPDX-License-Identifier: MIT
// Original Caskrun Vulkan presentation diagnostic. No game or AnyPS5 code.
#define WIN32_LEAN_AND_MEAN
#define VK_NO_PROTOTYPES
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <vulkan/vulkan.h>
#include <cstdio>
#include <cstring>
#include <cstdint>
#include <stdexcept>
#include <string>
#include <vector>
#include <algorithm>
#include <cstdlib>

static void yes(const char* name, bool value) { std::printf("%s=%s\n", name, value ? "yes" : "no"); }
static void checked(const char* name, VkResult r) {
    std::printf("%s=%d\n", name, int(r));
    if (r != VK_SUCCESS && r != VK_SUBOPTIMAL_KHR) throw std::runtime_error(name);
}
static void need(bool v, const char* reason) { if (!v) throw std::runtime_error(reason); }
static bool contains(const std::vector<VkExtensionProperties>& v, const char* name) {
    return std::any_of(v.begin(), v.end(), [name](const auto& x) { return std::strcmp(x.extensionName, name) == 0; });
}
static LRESULT CALLBACK windowProcedure(HWND w, UINT msg, WPARAM a, LPARAM b) {
    if (msg == WM_CLOSE) { PostQuitMessage(0); return 0; }
    return DefWindowProcW(w, msg, a, b);
}
#define INSTANCE_FUNCS(X) \
 X(DestroyInstance) X(EnumeratePhysicalDevices) X(GetPhysicalDeviceFeatures2) \
 X(GetPhysicalDeviceProperties2) X(EnumerateDeviceExtensionProperties) \
 X(GetPhysicalDeviceQueueFamilyProperties) X(GetPhysicalDeviceMemoryProperties) \
 X(GetPhysicalDeviceFormatProperties) X(GetPhysicalDeviceImageFormatProperties) \
 X(CreateDevice) X(GetDeviceProcAddr) X(CreateWin32SurfaceKHR) X(DestroySurfaceKHR) \
 X(GetPhysicalDeviceSurfaceSupportKHR) X(GetPhysicalDeviceSurfaceCapabilitiesKHR) \
 X(GetPhysicalDeviceSurfaceFormatsKHR) X(GetPhysicalDeviceSurfacePresentModesKHR)
#define DEVICE_FUNCS(X) \
 X(DestroyDevice) X(GetDeviceQueue) X(DeviceWaitIdle) X(CreateSwapchainKHR) X(DestroySwapchainKHR) \
 X(GetSwapchainImagesKHR) X(AcquireNextImageKHR) X(CreateBuffer) X(DestroyBuffer) \
 X(GetBufferMemoryRequirements) X(AllocateMemory) X(FreeMemory) X(BindBufferMemory) \
 X(MapMemory) X(UnmapMemory) X(CreateImage) X(DestroyImage) X(GetImageMemoryRequirements) \
 X(BindImageMemory) X(GetImageSubresourceLayout) X(CreateCommandPool) X(DestroyCommandPool) \
 X(AllocateCommandBuffers) X(ResetCommandBuffer) X(BeginCommandBuffer) X(EndCommandBuffer) \
 X(CmdPipelineBarrier) X(CmdCopyBufferToImage) X(CmdCopyImageToBuffer) X(CreateSemaphore) \
 X(DestroySemaphore) X(CreateFence) X(DestroyFence) X(WaitForFences) X(ResetFences) \
 X(QueueSubmit) X(QueuePresentKHR)
#define DECLARE(N) PFN_vk##N N = nullptr;
struct Context {
    HMODULE loader = nullptr; HWND window = nullptr;
    PFN_vkGetInstanceProcAddr get = nullptr;
    VkInstance instance = VK_NULL_HANDLE; VkPhysicalDevice physical = VK_NULL_HANDLE;
    VkSurfaceKHR surface = VK_NULL_HANDLE; VkDevice device = VK_NULL_HANDLE;
    VkQueue queue = VK_NULL_HANDLE; uint32_t family = UINT32_MAX;
    VkSwapchainKHR swapchain = VK_NULL_HANDLE; VkCommandPool pool = VK_NULL_HANDLE;
    VkSemaphore acquired = VK_NULL_HANDLE, rendered = VK_NULL_HANDLE;
    VkFence fence = VK_NULL_HANDLE; VkBuffer upload = VK_NULL_HANDLE, readback = VK_NULL_HANDLE;
    VkDeviceMemory uploadMemory = VK_NULL_HANDLE, readMemory = VK_NULL_HANDLE, imageMemory = VK_NULL_HANDLE;
    VkImage offscreen = VK_NULL_HANDLE; bool uploadMapped = false, readMapped = false;
    INSTANCE_FUNCS(DECLARE)
    DEVICE_FUNCS(DECLARE)
    ~Context() {
        if (device && DeviceWaitIdle) DeviceWaitIdle(device);
        if (uploadMapped && UnmapMemory) UnmapMemory(device, uploadMemory);
        if (readMapped && UnmapMemory) UnmapMemory(device, readMemory);
        if (device) {
            if (fence && DestroyFence) DestroyFence(device, fence, nullptr);
            if (acquired && DestroySemaphore) DestroySemaphore(device, acquired, nullptr);
            if (rendered && DestroySemaphore) DestroySemaphore(device, rendered, nullptr);
            if (pool && DestroyCommandPool) DestroyCommandPool(device, pool, nullptr);
            if (offscreen && DestroyImage) DestroyImage(device, offscreen, nullptr);
            if (upload && DestroyBuffer) DestroyBuffer(device, upload, nullptr);
            if (readback && DestroyBuffer) DestroyBuffer(device, readback, nullptr);
            if (uploadMemory && FreeMemory) FreeMemory(device, uploadMemory, nullptr);
            if (readMemory && FreeMemory) FreeMemory(device, readMemory, nullptr);
            if (imageMemory && FreeMemory) FreeMemory(device, imageMemory, nullptr);
            if (swapchain && DestroySwapchainKHR) DestroySwapchainKHR(device, swapchain, nullptr);
            if (DestroyDevice) DestroyDevice(device, nullptr);
        }
        if (surface && DestroySurfaceKHR) DestroySurfaceKHR(instance, surface, nullptr);
        if (instance && DestroyInstance) DestroyInstance(instance, nullptr);
        if (window) DestroyWindow(window);
        if (loader) FreeLibrary(loader);
    }
    uint32_t memory(uint32_t allowed, VkMemoryPropertyFlags wanted) {
        VkPhysicalDeviceMemoryProperties p{}; GetPhysicalDeviceMemoryProperties(physical, &p);
        for (uint32_t i=0; i<p.memoryTypeCount; ++i)
            if ((allowed & (1u<<i)) && (p.memoryTypes[i].propertyFlags & wanted) == wanted) return i;
        throw std::runtime_error("required_memory_type_unavailable");
    }
    void buffer(VkDeviceSize bytes, VkBufferUsageFlags usage, VkBuffer& b, VkDeviceMemory& m) {
        VkBufferCreateInfo info{}; info.sType=VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO;
        info.size=bytes; info.usage=usage; info.sharingMode=VK_SHARING_MODE_EXCLUSIVE;
        checked("create_staging_buffer", CreateBuffer(device, &info, nullptr, &b));
        VkMemoryRequirements req{}; GetBufferMemoryRequirements(device, b, &req);
        VkMemoryAllocateInfo alloc{}; alloc.sType=VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
        alloc.allocationSize=req.size;
        alloc.memoryTypeIndex=memory(req.memoryTypeBits, VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT|VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
        checked("allocate_staging_memory", AllocateMemory(device, &alloc, nullptr, &m));
        checked("bind_staging_memory", BindBufferMemory(device, b, m, 0));
    }
};
#undef DECLARE

static void initialize(Context& c, bool allowPortability) {
    c.loader=LoadLibraryW(L"vulkan-1.dll"); need(c.loader != nullptr, "vulkan_loader_unavailable");
    auto address=GetProcAddress(c.loader, "vkGetInstanceProcAddr");
    static_assert(sizeof(address)==sizeof(c.get)); std::memcpy(&c.get, &address, sizeof(c.get));
    need(c.get != nullptr, "vulkan_get_proc_missing");
    auto enumerate=reinterpret_cast<PFN_vkEnumerateInstanceExtensionProperties>(c.get(nullptr,"vkEnumerateInstanceExtensionProperties"));
    auto create=reinterpret_cast<PFN_vkCreateInstance>(c.get(nullptr,"vkCreateInstance"));
    need(enumerate && create,"vulkan_globals_missing");
    uint32_t count=0; checked("instance_extension_count_query_result",enumerate(nullptr,&count,nullptr)); need(count<=512,"too_many_instance_extensions");
    std::printf("instance_extension_count=%u\n",count);
    std::vector<VkExtensionProperties> extensions(count); checked("instance_extensions",enumerate(nullptr,&count,extensions.data()));
    need(contains(extensions,VK_KHR_SURFACE_EXTENSION_NAME) && contains(extensions,VK_KHR_WIN32_SURFACE_EXTENSION_NAME),"win32_surface_extensions_missing");
    std::vector<const char*> enabled{VK_KHR_SURFACE_EXTENSION_NAME,VK_KHR_WIN32_SURFACE_EXTENSION_NAME};
    bool portable=allowPortability && contains(extensions,VK_KHR_PORTABILITY_ENUMERATION_EXTENSION_NAME);
    yes("portability_enumeration_enabled",portable);
    if (portable) enabled.push_back(VK_KHR_PORTABILITY_ENUMERATION_EXTENSION_NAME);
    VkApplicationInfo app{}; app.sType=VK_STRUCTURE_TYPE_APPLICATION_INFO;
    app.pApplicationName="Caskrun original presentation diagnostic"; app.apiVersion=VK_API_VERSION_1_1;
    VkInstanceCreateInfo info{}; info.sType=VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO; info.pApplicationInfo=&app;
    info.enabledExtensionCount=uint32_t(enabled.size()); info.ppEnabledExtensionNames=enabled.data();
    if(portable) info.flags=VK_INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR;
    checked("create_instance",create(&info,nullptr,&c.instance));
#define LOAD_I(N) c.N=reinterpret_cast<PFN_vk##N>(c.get(c.instance,"vk" #N)); need(c.N!=nullptr,"missing_instance_" #N);
    INSTANCE_FUNCS(LOAD_I)
#undef LOAD_I
    WNDCLASSW klass{}; klass.lpfnWndProc=windowProcedure; klass.hInstance=GetModuleHandleW(nullptr);
    klass.lpszClassName=L"CaskrunOriginalVulkanPresentation"; klass.hCursor=LoadCursorW(nullptr,MAKEINTRESOURCEW(32512));
    need(RegisterClassW(&klass)!=0,"window_class_failed");
    c.window=CreateWindowExW(0,klass.lpszClassName,L"Caskrun Vulkan presentation test",WS_OVERLAPPEDWINDOW,
                              CW_USEDEFAULT,CW_USEDEFAULT,640,440,nullptr,nullptr,klass.hInstance,nullptr);
    need(c.window!=nullptr,"window_creation_failed"); ShowWindow(c.window,SW_SHOW); UpdateWindow(c.window);
    VkWin32SurfaceCreateInfoKHR surface{}; surface.sType=VK_STRUCTURE_TYPE_WIN32_SURFACE_CREATE_INFO_KHR;
    surface.hinstance=klass.hInstance; surface.hwnd=c.window;
    checked("create_win32_surface",c.CreateWin32SurfaceKHR(c.instance,&surface,nullptr,&c.surface));
    count=0; checked("physical_device_count_query_result",c.EnumeratePhysicalDevices(c.instance,&count,nullptr)); need(count>0&&count<=16,"physical_devices_unavailable");
    std::printf("physical_device_count=%u\n",count);
    std::vector<VkPhysicalDevice> devices(count); checked("physical_devices",c.EnumeratePhysicalDevices(c.instance,&count,devices.data()));
    c.physical=devices.front();
    c.GetPhysicalDeviceQueueFamilyProperties(c.physical,&count,nullptr); need(count>0&&count<=512,"queue_family_invalid");
    std::vector<VkQueueFamilyProperties> families(count); c.GetPhysicalDeviceQueueFamilyProperties(c.physical,&count,families.data());
    for(uint32_t i=0;i<count;++i) {
        VkBool32 support=VK_FALSE; checked("surface_queue_support",c.GetPhysicalDeviceSurfaceSupportKHR(c.physical,i,c.surface,&support));
        if(support&&families[i].queueCount&&(families[i].queueFlags&VK_QUEUE_GRAPHICS_BIT)){c.family=i;break;}
    }
    need(c.family!=UINT32_MAX,"graphics_present_queue_unavailable");
    count=0; checked("device_extension_count_query_result",c.EnumerateDeviceExtensionProperties(c.physical,nullptr,&count,nullptr)); need(count<=512,"too_many_device_extensions");
    std::printf("device_extension_count=%u\n",count);
    std::vector<VkExtensionProperties> dext(count); checked("device_extensions",c.EnumerateDeviceExtensionProperties(c.physical,nullptr,&count,dext.data()));
    std::vector<const char*> req{VK_KHR_SWAPCHAIN_EXTENSION_NAME,VK_KHR_8BIT_STORAGE_EXTENSION_NAME,
                                VK_KHR_BUFFER_DEVICE_ADDRESS_EXTENSION_NAME,VK_KHR_SHADER_FLOAT_CONTROLS_EXTENSION_NAME};
    for(const auto* e:req) need(contains(dext,e),e);
    if(contains(dext,"VK_KHR_portability_subset")) req.push_back("VK_KHR_portability_subset");
    VkPhysicalDevice8BitStorageFeatures storage{}; storage.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_8BIT_STORAGE_FEATURES;
    VkPhysicalDeviceBufferDeviceAddressFeatures bda{}; bda.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_BUFFER_DEVICE_ADDRESS_FEATURES; storage.pNext=&bda;
    VkPhysicalDeviceRobustness2FeaturesEXT robust{}; robust.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ROBUSTNESS_2_FEATURES_EXT;
    if(contains(dext,VK_EXT_ROBUSTNESS_2_EXTENSION_NAME)) bda.pNext=&robust;
    VkPhysicalDeviceFeatures2 features{}; features.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2; features.pNext=&storage;
    c.GetPhysicalDeviceFeatures2(c.physical,&features);
    const auto& avail=features.features;
    yes("reported_null_descriptor",robust.nullDescriptor);
    need(avail.shaderInt64&&avail.vertexPipelineStoresAndAtomics&&avail.fragmentStoresAndAtomics&&avail.samplerAnisotropy&&avail.textureCompressionBC,
         "required_core_features_missing");
    need(storage.storageBuffer8BitAccess&&bda.bufferDeviceAddress,"required_chained_features_missing");
    VkPhysicalDeviceFloatControlsProperties floats{}; floats.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FLOAT_CONTROLS_PROPERTIES;
    VkPhysicalDeviceProperties2 props{}; props.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2; props.pNext=&floats;
    c.GetPhysicalDeviceProperties2(c.physical,&props);
    need(props.properties.apiVersion>=VK_API_VERSION_1_1&&floats.shaderSignedZeroInfNanPreserveFloat32,"required_float_control_missing");
    // Enable required features, plus the same available core options the pinned engine selects.
    // nullDescriptor is truthfully omitted; this does not assert engine compatibility.
    VkPhysicalDeviceFeatures core{};
    core.shaderInt64=VK_TRUE; core.vertexPipelineStoresAndAtomics=VK_TRUE; core.fragmentStoresAndAtomics=VK_TRUE;
    core.samplerAnisotropy=VK_TRUE; core.textureCompressionBC=VK_TRUE;
    core.shaderFloat64=avail.shaderFloat64&&floats.shaderSignedZeroInfNanPreserveFloat64;
#define CASKRUN_OPTIONAL_CORE(N) core.N=avail.N;
    CASKRUN_OPTIONAL_CORE(tessellationShader) CASKRUN_OPTIONAL_CORE(robustBufferAccess) CASKRUN_OPTIONAL_CORE(drawIndirectFirstInstance) CASKRUN_OPTIONAL_CORE(multiDrawIndirect)
    CASKRUN_OPTIONAL_CORE(depthClamp) CASKRUN_OPTIONAL_CORE(depthBounds) CASKRUN_OPTIONAL_CORE(depthBiasClamp) CASKRUN_OPTIONAL_CORE(occlusionQueryPrecise)
    CASKRUN_OPTIONAL_CORE(shaderStorageImageMultisample) CASKRUN_OPTIONAL_CORE(shaderStorageImageWriteWithoutFormat) CASKRUN_OPTIONAL_CORE(shaderStorageImageReadWithoutFormat)
    CASKRUN_OPTIONAL_CORE(shaderImageGatherExtended) CASKRUN_OPTIONAL_CORE(shaderResourceMinLod) CASKRUN_OPTIONAL_CORE(sampleRateShading) CASKRUN_OPTIONAL_CORE(geometryShader)
    CASKRUN_OPTIONAL_CORE(shaderClipDistance) CASKRUN_OPTIONAL_CORE(shaderSampledImageArrayDynamicIndexing) CASKRUN_OPTIONAL_CORE(shaderStorageImageArrayDynamicIndexing)
#undef CASKRUN_OPTIONAL_CORE
    storage={}; storage.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_8BIT_STORAGE_FEATURES; storage.storageBuffer8BitAccess=VK_TRUE;
    bda={}; bda.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_BUFFER_DEVICE_ADDRESS_FEATURES; bda.bufferDeviceAddress=VK_TRUE; storage.pNext=&bda;
    float priority=1.f; VkDeviceQueueCreateInfo qi{}; qi.sType=VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO;
    qi.queueFamilyIndex=c.family; qi.queueCount=1; qi.pQueuePriorities=&priority;
    VkDeviceCreateInfo device{}; device.sType=VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO; device.pNext=&storage; device.pEnabledFeatures=&core;
    device.queueCreateInfoCount=1; device.pQueueCreateInfos=&qi; device.enabledExtensionCount=uint32_t(req.size()); device.ppEnabledExtensionNames=req.data();
    checked("required_features_without_null_create_device",c.CreateDevice(c.physical,&device,nullptr,&c.device));
#define LOAD_D(N) c.N=reinterpret_cast<PFN_vk##N>(c.GetDeviceProcAddr(c.device,"vk" #N)); need(c.N!=nullptr,"missing_device_" #N);
    DEVICE_FUNCS(LOAD_D)
#undef LOAD_D
    c.GetDeviceQueue(c.device,c.family,0,&c.queue); need(c.queue!=VK_NULL_HANDLE,"queue_missing");
}

static void present(Context& c, unsigned seconds) {
    VkFormatProperties format{}; c.GetPhysicalDeviceFormatProperties(c.physical,VK_FORMAT_B8G8R8A8_UNORM,&format);
    yes("bgra_optimal_transfer_destination",format.optimalTilingFeatures&VK_FORMAT_FEATURE_TRANSFER_DST_BIT);
    yes("bgra_optimal_transfer_source",format.optimalTilingFeatures&VK_FORMAT_FEATURE_TRANSFER_SRC_BIT);
    yes("bgra_linear_transfer_source",format.linearTilingFeatures&VK_FORMAT_FEATURE_TRANSFER_SRC_BIT);
    VkImageFormatProperties imageProperties{};
    VkResult linearQuery=c.GetPhysicalDeviceImageFormatProperties(c.physical,VK_FORMAT_B8G8R8A8_UNORM,VK_IMAGE_TYPE_2D,
                       VK_IMAGE_TILING_LINEAR,VK_IMAGE_USAGE_TRANSFER_SRC_BIT,0,&imageProperties);
    std::printf("linear_bgra_transfer_image_format=%d\n",int(linearQuery));
    VkSurfaceCapabilitiesKHR cap{}; checked("surface_capabilities",c.GetPhysicalDeviceSurfaceCapabilitiesKHR(c.physical,c.surface,&cap));
    yes("surface_transfer_destination",cap.supportedUsageFlags&VK_IMAGE_USAGE_TRANSFER_DST_BIT);
    yes("surface_opaque_alpha",cap.supportedCompositeAlpha&VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR);
    yes("surface_identity_transform",cap.supportedTransforms&VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR);
    need((cap.supportedUsageFlags&VK_IMAGE_USAGE_TRANSFER_DST_BIT)&&(cap.supportedCompositeAlpha&VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR)&&
         (cap.supportedTransforms&VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR),"required_surface_policy_unavailable");
    uint32_t count=0; checked("surface_format_count_query_result",c.GetPhysicalDeviceSurfaceFormatsKHR(c.physical,c.surface,&count,nullptr)); need(count>0&&count<=512,"surface_format_bounds");
    std::printf("surface_format_count=%u\n",count);
    std::vector<VkSurfaceFormatKHR> formats(count); checked("surface_formats",c.GetPhysicalDeviceSurfaceFormatsKHR(c.physical,c.surface,&count,formats.data()));
    need(std::any_of(formats.begin(),formats.end(),[](const auto& f){return f.format==VK_FORMAT_B8G8R8A8_UNORM&&f.colorSpace==VK_COLOR_SPACE_SRGB_NONLINEAR_KHR;}),"bgra_unorm_srgb_surface_missing");
    yes("surface_bgra_unorm_srgb",true);
    VkExtent2D extent=cap.currentExtent;
    if(extent.width==UINT32_MAX){ RECT r{}; GetClientRect(c.window,&r); extent={uint32_t(r.right),uint32_t(r.bottom)}; }
    need(extent.width>=64&&extent.height>=64&&extent.width<=4096&&extent.height<=4096,"extent_bounds");
    VkSwapchainCreateInfoKHR swap{}; swap.sType=VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR; swap.surface=c.surface;
    swap.minImageCount=cap.minImageCount; swap.imageFormat=VK_FORMAT_B8G8R8A8_UNORM; swap.imageColorSpace=VK_COLOR_SPACE_SRGB_NONLINEAR_KHR;
    swap.imageExtent=extent; swap.imageArrayLayers=1; swap.imageUsage=VK_IMAGE_USAGE_TRANSFER_DST_BIT; swap.imageSharingMode=VK_SHARING_MODE_EXCLUSIVE;
    swap.preTransform=VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR; swap.compositeAlpha=VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR;
    swap.presentMode=VK_PRESENT_MODE_FIFO_KHR; swap.clipped=VK_FALSE;
    checked("create_swapchain",c.CreateSwapchainKHR(c.device,&swap,nullptr,&c.swapchain));
    count=0; checked("swapchain_image_count_query_result",c.GetSwapchainImagesKHR(c.device,c.swapchain,&count,nullptr)); need(count>0&&count<=16,"swapchain_image_bounds");
    std::printf("swapchain_image_count=%u\n",count);
    std::vector<VkImage> images(count); checked("swapchain_images",c.GetSwapchainImagesKHR(c.device,c.swapchain,&count,images.data()));
    const VkDeviceSize bytes=VkDeviceSize(extent.width)*extent.height*4;
    c.buffer(bytes,VK_BUFFER_USAGE_TRANSFER_SRC_BIT,c.upload,c.uploadMemory);
    void* mapped=nullptr; checked("map_upload_memory",c.MapMemory(c.device,c.uploadMemory,0,bytes,0,&mapped)); c.uploadMapped=true;
    auto* pixels=static_cast<uint32_t*>(mapped);
    for(uint32_t y=0;y<extent.height;++y) for(uint32_t x=0;x<extent.width;++x)
        pixels[size_t(y)*extent.width+x]=((x/48+y/48)&1)?0xff1fcee8u:0xff743ef0u;
    VkCommandPoolCreateInfo pool{}; pool.sType=VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    pool.flags=VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT; pool.queueFamilyIndex=c.family;
    checked("create_command_pool",c.CreateCommandPool(c.device,&pool,nullptr,&c.pool));
    VkCommandBufferAllocateInfo alloc{}; alloc.sType=VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO; alloc.commandPool=c.pool; alloc.level=VK_COMMAND_BUFFER_LEVEL_PRIMARY; alloc.commandBufferCount=1;
    VkCommandBuffer command=VK_NULL_HANDLE; checked("allocate_command_buffer",c.AllocateCommandBuffers(c.device,&alloc,&command));
    VkSemaphoreCreateInfo sem{}; sem.sType=VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO;
    checked("create_acquire_semaphore",c.CreateSemaphore(c.device,&sem,nullptr,&c.acquired));
    checked("create_render_semaphore",c.CreateSemaphore(c.device,&sem,nullptr,&c.rendered));
    VkFenceCreateInfo fence{}; fence.sType=VK_STRUCTURE_TYPE_FENCE_CREATE_INFO; fence.flags=VK_FENCE_CREATE_SIGNALED_BIT;
    checked("create_fence",c.CreateFence(c.device,&fence,nullptr,&c.fence));
    // Test actual GPU upload/readback into the optimal BGRA image form the engine uses.
    // Guest "linear" here describes contiguous CPU pixels, not a Vulkan linear-tiled image.
    VkImageCreateInfo imageInfo{}; imageInfo.sType=VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
    imageInfo.imageType=VK_IMAGE_TYPE_2D; imageInfo.format=VK_FORMAT_B8G8R8A8_UNORM;
    imageInfo.extent={64,64,1}; imageInfo.mipLevels=1; imageInfo.arrayLayers=1;
    imageInfo.samples=VK_SAMPLE_COUNT_1_BIT; imageInfo.tiling=VK_IMAGE_TILING_OPTIMAL;
    imageInfo.usage=VK_IMAGE_USAGE_TRANSFER_SRC_BIT|VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    imageInfo.sharingMode=VK_SHARING_MODE_EXCLUSIVE; imageInfo.initialLayout=VK_IMAGE_LAYOUT_UNDEFINED;
    checked("create_optimal_bgra_image",c.CreateImage(c.device,&imageInfo,nullptr,&c.offscreen));
    VkMemoryRequirements imreq{}; c.GetImageMemoryRequirements(c.device,c.offscreen,&imreq);
    VkMemoryAllocateInfo imalloc{}; imalloc.sType=VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO; imalloc.allocationSize=imreq.size;
    imalloc.memoryTypeIndex=c.memory(imreq.memoryTypeBits,0);
    checked("allocate_bgra_image_memory",c.AllocateMemory(c.device,&imalloc,nullptr,&c.imageMemory));
    checked("bind_bgra_image_memory",c.BindImageMemory(c.device,c.offscreen,c.imageMemory,0));
    c.buffer(64*64*4,VK_BUFFER_USAGE_TRANSFER_DST_BIT,c.readback,c.readMemory);
    VkCommandBufferBeginInfo roundBegin{}; roundBegin.sType=VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    checked("begin_roundtrip_command",c.BeginCommandBuffer(command,&roundBegin));
    VkImageMemoryBarrier roundBarrier{}; roundBarrier.sType=VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    roundBarrier.oldLayout=VK_IMAGE_LAYOUT_UNDEFINED; roundBarrier.newLayout=VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL;
    roundBarrier.dstAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT; roundBarrier.image=c.offscreen;
    roundBarrier.srcQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED; roundBarrier.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;
    roundBarrier.subresourceRange={VK_IMAGE_ASPECT_COLOR_BIT,0,1,0,1};
    c.CmdPipelineBarrier(command,VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,VK_PIPELINE_STAGE_TRANSFER_BIT,0,0,nullptr,0,nullptr,1,&roundBarrier);
    VkBufferImageCopy uploadCopy{}; uploadCopy.bufferRowLength=extent.width;
    uploadCopy.imageSubresource={VK_IMAGE_ASPECT_COLOR_BIT,0,0,1}; uploadCopy.imageExtent={64,64,1};
    c.CmdCopyBufferToImage(command,c.upload,c.offscreen,VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,1,&uploadCopy);
    roundBarrier.oldLayout=VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL; roundBarrier.newLayout=VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
    roundBarrier.srcAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT; roundBarrier.dstAccessMask=VK_ACCESS_TRANSFER_READ_BIT;
    c.CmdPipelineBarrier(command,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_TRANSFER_BIT,0,0,nullptr,0,nullptr,1,&roundBarrier);
    uploadCopy.bufferRowLength=0;
    c.CmdCopyImageToBuffer(command,c.offscreen,VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,c.readback,1,&uploadCopy);
    VkBufferMemoryBarrier readBarrier{}; readBarrier.sType=VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER;
    readBarrier.srcAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT; readBarrier.dstAccessMask=VK_ACCESS_HOST_READ_BIT;
    readBarrier.srcQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED; readBarrier.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;
    readBarrier.buffer=c.readback; readBarrier.size=VK_WHOLE_SIZE;
    c.CmdPipelineBarrier(command,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_HOST_BIT,0,0,nullptr,1,&readBarrier,0,nullptr);
    checked("end_roundtrip_command",c.EndCommandBuffer(command));
    VkSubmitInfo roundSubmit{}; roundSubmit.sType=VK_STRUCTURE_TYPE_SUBMIT_INFO; roundSubmit.commandBufferCount=1; roundSubmit.pCommandBuffers=&command;
    checked("submit_upload_readback",c.QueueSubmit(c.queue,1,&roundSubmit,VK_NULL_HANDLE));
    checked("upload_readback_wait_idle",c.DeviceWaitIdle(c.device));
    void* returned=nullptr; checked("map_readback_memory",c.MapMemory(c.device,c.readMemory,0,64*64*4,0,&returned)); c.readMapped=true;
    const auto* readPixels=static_cast<const uint32_t*>(returned); unsigned matched=0;
    for(uint32_t y=0;y<64;++y) for(uint32_t x=0;x<64;++x) if(readPixels[size_t(y)*64+x]==pixels[size_t(y)*extent.width+x])++matched;
    std::printf("bgra_roundtrip_matched_pixels=%u/4096\n",matched); need(matched==4096,"bgra_upload_readback_mismatch");
    std::vector<bool> used(images.size(),false); uint32_t frames=0;
    const uint64_t until=GetTickCount64()+uint64_t(seconds)*1000;
    while(GetTickCount64()<until) {
        MSG msg{}; bool closed=false; while(PeekMessageW(&msg,nullptr,0,0,PM_REMOVE)){if(msg.message==WM_QUIT){closed=true;break;}TranslateMessage(&msg);DispatchMessageW(&msg);}
        if(closed) break;
        need(c.WaitForFences(c.device,1,&c.fence,VK_TRUE,5000000000ull)==VK_SUCCESS,"frame_fence_wait_failed");
        uint32_t image=0; VkResult r=c.AcquireNextImageKHR(c.device,c.swapchain,5000000000ull,c.acquired,VK_NULL_HANDLE,&image);
        need(r==VK_SUCCESS||r==VK_SUBOPTIMAL_KHR,"acquire_failed"); need(image<images.size(),"image_index_out_of_bounds");
        need(c.ResetFences(c.device,1,&c.fence)==VK_SUCCESS,"reset_fence_failed");
        need(c.ResetCommandBuffer(command,0)==VK_SUCCESS,"reset_command_failed");
        VkCommandBufferBeginInfo begin{}; begin.sType=VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO; begin.flags=VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        need(c.BeginCommandBuffer(command,&begin)==VK_SUCCESS,"begin_command_failed");
        VkImageMemoryBarrier barrier{}; barrier.sType=VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
        barrier.oldLayout=used[image]?VK_IMAGE_LAYOUT_PRESENT_SRC_KHR:VK_IMAGE_LAYOUT_UNDEFINED;
        barrier.newLayout=VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL; barrier.dstAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT;
        barrier.srcQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED; barrier.dstQueueFamilyIndex=VK_QUEUE_FAMILY_IGNORED;
        barrier.image=images[image]; barrier.subresourceRange={VK_IMAGE_ASPECT_COLOR_BIT,0,1,0,1};
        c.CmdPipelineBarrier(command,VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,VK_PIPELINE_STAGE_TRANSFER_BIT,0,0,nullptr,0,nullptr,1,&barrier);
        VkBufferImageCopy copy{}; copy.imageSubresource={VK_IMAGE_ASPECT_COLOR_BIT,0,0,1}; copy.imageExtent={extent.width,extent.height,1};
        c.CmdCopyBufferToImage(command,c.upload,images[image],VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,1,&copy);
        barrier.oldLayout=VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL; barrier.newLayout=VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;
        barrier.srcAccessMask=VK_ACCESS_TRANSFER_WRITE_BIT; barrier.dstAccessMask=0;
        c.CmdPipelineBarrier(command,VK_PIPELINE_STAGE_TRANSFER_BIT,VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,0,0,nullptr,0,nullptr,1,&barrier);
        need(c.EndCommandBuffer(command)==VK_SUCCESS,"end_command_failed");
        const VkPipelineStageFlags wait=VK_PIPELINE_STAGE_TRANSFER_BIT;
        VkSubmitInfo submit{}; submit.sType=VK_STRUCTURE_TYPE_SUBMIT_INFO; submit.waitSemaphoreCount=1; submit.pWaitSemaphores=&c.acquired;
        submit.pWaitDstStageMask=&wait; submit.commandBufferCount=1; submit.pCommandBuffers=&command; submit.signalSemaphoreCount=1; submit.pSignalSemaphores=&c.rendered;
        need(c.QueueSubmit(c.queue,1,&submit,c.fence)==VK_SUCCESS,"queue_submit_failed");
        VkPresentInfoKHR pi{}; pi.sType=VK_STRUCTURE_TYPE_PRESENT_INFO_KHR; pi.waitSemaphoreCount=1; pi.pWaitSemaphores=&c.rendered;
        pi.swapchainCount=1; pi.pSwapchains=&c.swapchain; pi.pImageIndices=&image;
        r=c.QueuePresentKHR(c.queue,&pi); need(r==VK_SUCCESS||r==VK_SUBOPTIMAL_KHR,"queue_present_failed");
        // Queue idle serializes binary-semaphore reuse for this diagnostic. No performance claim.
        need(c.DeviceWaitIdle(c.device)==VK_SUCCESS,"frame_device_wait_failed");
        used[image]=true; ++frames;
    }
    std::printf("presented_frames=%u\n",frames); need(frames>0,"no_frames_presented");
    std::printf("extent=%ux%u\n",extent.width,extent.height);
    std::puts("scope=required-features-without-null-and-buffer-upload-to-real-win32-swapchain-no-gameplay");
}
int main(int argc,char** argv) {
    std::setvbuf(stdout,nullptr,_IONBF,0); std::puts("probe=original-anyps5-presentation-v2");
    unsigned seconds=5;
    if(argc>=2){ char* end=nullptr; auto n=std::strtoul(argv[1],&end,10); if(!end||*end||n<1||n>120)return 64; seconds=unsigned(n); }
    bool allowPortability=true;
    if(argc==3) { if(std::strcmp(argv[2],"--no-portability")!=0)return 64; allowPortability=false; }
    if(argc>3)return 64;
    try { Context c; initialize(c,allowPortability); present(c,seconds); return 0; }
    catch(const std::exception& e){std::printf("failure=%s\n",e.what());return 1;}
}
