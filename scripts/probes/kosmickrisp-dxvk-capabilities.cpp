// SPDX-License-Identifier: MIT
// Original Caskrun bounded Vulkan feature and compute/readback diagnostic.
// This does not establish D3D, swapchain, title or gameplay compatibility.
#include <vulkan/vulkan.h>
#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <vector>

#define CHECK(call) do { VkResult r = (call); if (r != VK_SUCCESS) { \
  std::printf("failure=%s:%d\n", #call, r); return 1; } } while (0)
static void flag(const char *name, bool value) {
  std::printf("%s=%s\n", name, value ? "yes" : "no");
}
int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc != 2) return 2;
  std::puts("probe=caskrun-kosmickrisp-dxvk-feature-compute-v1");
  uint32_t version = 0;
  CHECK(vkEnumerateInstanceVersion(&version));
  std::printf("loader_api=%u.%u.%u\n", VK_API_VERSION_MAJOR(version),
    VK_API_VERSION_MINOR(version), VK_API_VERSION_PATCH(version));
  VkApplicationInfo app{VK_STRUCTURE_TYPE_APPLICATION_INFO};
  app.pApplicationName = "Caskrun Vulkan offscreen diagnostic";
  app.apiVersion = VK_API_VERSION_1_4;
  VkInstanceCreateInfo ci{VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO};
  ci.pApplicationInfo = &app;
  VkInstance instance{};
  CHECK(vkCreateInstance(&ci, nullptr, &instance));
  uint32_t n = 0;
  CHECK(vkEnumeratePhysicalDevices(instance, &n, nullptr));
  if (!n || n > 8) return 1;
  std::vector<VkPhysicalDevice> devices(n);
  CHECK(vkEnumeratePhysicalDevices(instance, &n, devices.data()));
  auto physical = devices[0];
  VkPhysicalDeviceProperties props{};
  vkGetPhysicalDeviceProperties(physical, &props);
  std::printf("device_api=%u.%u.%u\n", VK_API_VERSION_MAJOR(props.apiVersion),
    VK_API_VERSION_MINOR(props.apiVersion), VK_API_VERSION_PATCH(props.apiVersion));
  CHECK(vkEnumerateDeviceExtensionProperties(physical, nullptr, &n, nullptr));
  if (n > 1024) return 1;
  std::vector<VkExtensionProperties> extensions(n);
  CHECK(vkEnumerateDeviceExtensionProperties(physical, nullptr, &n, extensions.data()));
  bool required = props.apiVersion >= VK_API_VERSION_1_4 && props.limits.maxPushConstantsSize >= 256;
  for (const char *name : {"VK_EXT_depth_clip_enable", "VK_KHR_maintenance5",
       "VK_KHR_maintenance6", "VK_KHR_load_store_op_none", "VK_EXT_robustness2",
       "VK_EXT_transform_feedback"}) {
    bool found = std::any_of(extensions.begin(), extensions.end(), [name](const auto &e) {
      return std::string(e.extensionName) == name;
    });
    flag(name, found); required &= found;
  }
  VkPhysicalDeviceVulkan12Features f12{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES};
  VkPhysicalDeviceFeatures2 f{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2};
  f.pNext = &f12;
  vkGetPhysicalDeviceFeatures2(physical, &f);
  flag("shaderInt64", f.features.shaderInt64);
  flag("shaderInt16", f.features.shaderInt16);
  flag("shaderInt8", f12.shaderInt8);
  flag("descriptorIndexing", f12.descriptorIndexing);
  flag("scalarBlockLayout", f12.scalarBlockLayout);
  required &= f.features.shaderInt64 && f.features.shaderInt16 && f12.shaderInt8 &&
    f12.descriptorIndexing && f12.scalarBlockLayout;
  std::printf("maxPushConstantsSize=%u\n", props.limits.maxPushConstantsSize);
  flag("dxvk3_documented_requirements", required);
  vkGetPhysicalDeviceQueueFamilyProperties(physical, &n, nullptr);
  if (n > 128) return 1;
  std::vector<VkQueueFamilyProperties> families(n);
  vkGetPhysicalDeviceQueueFamilyProperties(physical, &n, families.data());
  uint32_t family = n;
  for (uint32_t i = 0; i < n; ++i)
    if (families[i].queueCount && (families[i].queueFlags & VK_QUEUE_COMPUTE_BIT)) { family = i; break; }
  if (family == n) return 1;
  float priority = 1;
  VkDeviceQueueCreateInfo qi{VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO};
  qi.queueFamilyIndex = family; qi.queueCount = 1; qi.pQueuePriorities = &priority;
  VkDeviceCreateInfo di{VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO};
  di.queueCreateInfoCount = 1; di.pQueueCreateInfos = &qi;
  VkDevice device{};
  CHECK(vkCreateDevice(physical, &di, nullptr, &device));
  VkQueue queue{}; vkGetDeviceQueue(device, family, 0, &queue);
  VkBufferCreateInfo bi{VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO};
  bi.size = 256 * sizeof(uint32_t); bi.usage = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
  VkBuffer buffer{}; CHECK(vkCreateBuffer(device, &bi, nullptr, &buffer));
  VkMemoryRequirements mr{}; vkGetBufferMemoryRequirements(device, buffer, &mr);
  VkPhysicalDeviceMemoryProperties mp{}; vkGetPhysicalDeviceMemoryProperties(physical, &mp);
  uint32_t type = mp.memoryTypeCount;
  for (uint32_t i = 0; i < mp.memoryTypeCount; ++i)
    if ((mr.memoryTypeBits & (1u << i)) && (mp.memoryTypes[i].propertyFlags &
        (VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT)) ==
        (VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT)) { type = i; break; }
  if (type == mp.memoryTypeCount) return 1;
  VkMemoryAllocateInfo ai{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
  ai.allocationSize = mr.size; ai.memoryTypeIndex = type;
  VkDeviceMemory memory{}; CHECK(vkAllocateMemory(device, &ai, nullptr, &memory));
  CHECK(vkBindBufferMemory(device, buffer, memory, 0));
  VkDescriptorSetLayoutBinding binding{};
  binding.binding = 0; binding.descriptorCount = 1;
  binding.descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER; binding.stageFlags = VK_SHADER_STAGE_COMPUTE_BIT;
  VkDescriptorSetLayoutCreateInfo li{VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO};
  li.bindingCount = 1; li.pBindings = &binding;
  VkDescriptorSetLayout setLayout{}; CHECK(vkCreateDescriptorSetLayout(device, &li, nullptr, &setLayout));
  VkPipelineLayoutCreateInfo pli{VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO};
  pli.setLayoutCount = 1; pli.pSetLayouts = &setLayout;
  VkPipelineLayout layout{}; CHECK(vkCreatePipelineLayout(device, &pli, nullptr, &layout));
  std::ifstream shader(argv[1], std::ios::binary | std::ios::ate);
  if (!shader) return 2;
  auto size = shader.tellg();
  if (size <= 0 || size > 1024 * 1024 || size % 4) return 2;
  std::vector<uint32_t> code(size_t(size) / 4); shader.seekg(0); shader.read(reinterpret_cast<char *>(code.data()), size);
  VkShaderModuleCreateInfo si{VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO};
  si.codeSize = size_t(size); si.pCode = code.data();
  VkShaderModule module{}; CHECK(vkCreateShaderModule(device, &si, nullptr, &module));
  VkComputePipelineCreateInfo pi{VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO};
  pi.layout = layout; pi.stage.sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO;
  pi.stage.stage = VK_SHADER_STAGE_COMPUTE_BIT; pi.stage.module = module; pi.stage.pName = "main";
  VkPipeline pipeline{}; CHECK(vkCreateComputePipelines(device, {}, 1, &pi, nullptr, &pipeline));
  VkDescriptorPoolSize poolSize{VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, 1};
  VkDescriptorPoolCreateInfo dpi{VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO};
  dpi.maxSets = 1; dpi.poolSizeCount = 1; dpi.pPoolSizes = &poolSize;
  VkDescriptorPool pool{}; CHECK(vkCreateDescriptorPool(device, &dpi, nullptr, &pool));
  VkDescriptorSetAllocateInfo dsi{VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO};
  dsi.descriptorPool = pool; dsi.descriptorSetCount = 1; dsi.pSetLayouts = &setLayout;
  VkDescriptorSet set{}; CHECK(vkAllocateDescriptorSets(device, &dsi, &set));
  VkDescriptorBufferInfo dbi{buffer, 0, bi.size};
  VkWriteDescriptorSet write{VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET};
  write.dstSet = set; write.descriptorCount = 1; write.descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER; write.pBufferInfo = &dbi;
  vkUpdateDescriptorSets(device, 1, &write, 0, nullptr);
  VkCommandPoolCreateInfo cpi{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO}; cpi.queueFamilyIndex = family;
  VkCommandPool commandPool{}; CHECK(vkCreateCommandPool(device, &cpi, nullptr, &commandPool));
  VkCommandBufferAllocateInfo cai{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
  cai.commandPool = commandPool; cai.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; cai.commandBufferCount = 1;
  VkCommandBuffer command{}; CHECK(vkAllocateCommandBuffers(device, &cai, &command));
  VkCommandBufferBeginInfo begin{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO}; CHECK(vkBeginCommandBuffer(command, &begin));
  vkCmdBindPipeline(command, VK_PIPELINE_BIND_POINT_COMPUTE, pipeline);
  vkCmdBindDescriptorSets(command, VK_PIPELINE_BIND_POINT_COMPUTE, layout, 0, 1, &set, 0, nullptr);
  vkCmdDispatch(command, 4, 1, 1);
  VkMemoryBarrier barrier{VK_STRUCTURE_TYPE_MEMORY_BARRIER};
  barrier.srcAccessMask = VK_ACCESS_SHADER_WRITE_BIT; barrier.dstAccessMask = VK_ACCESS_HOST_READ_BIT;
  vkCmdPipelineBarrier(command, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_PIPELINE_STAGE_HOST_BIT, 0, 1, &barrier, 0, nullptr, 0, nullptr);
  CHECK(vkEndCommandBuffer(command));
  VkFenceCreateInfo fi{VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
  VkFence fence{}; CHECK(vkCreateFence(device, &fi, nullptr, &fence));
  VkSubmitInfo submit{VK_STRUCTURE_TYPE_SUBMIT_INFO}; submit.commandBufferCount = 1; submit.pCommandBuffers = &command;
  CHECK(vkQueueSubmit(queue, 1, &submit, fence));
  CHECK(vkWaitForFences(device, 1, &fence, VK_TRUE, 5'000'000'000ull));
  void *mapped{}; CHECK(vkMapMemory(device, memory, 0, bi.size, 0, &mapped));
  bool correct = true;
  for (uint32_t i = 0; i < 256; ++i) correct &= static_cast<uint32_t *>(mapped)[i] == i * 3 + 7;
  vkUnmapMemory(device, memory);
  flag("compute_shader_256_values_readback", correct);
  vkDestroyFence(device, fence, nullptr); vkDestroyCommandPool(device, commandPool, nullptr);
  vkDestroyDescriptorPool(device, pool, nullptr); vkDestroyPipeline(device, pipeline, nullptr);
  vkDestroyShaderModule(device, module, nullptr); vkDestroyPipelineLayout(device, layout, nullptr);
  vkDestroyDescriptorSetLayout(device, setLayout, nullptr); vkDestroyBuffer(device, buffer, nullptr);
  vkFreeMemory(device, memory, nullptr); vkDestroyDevice(device, nullptr); vkDestroyInstance(instance, nullptr);
  return correct ? 0 : 1;
}
