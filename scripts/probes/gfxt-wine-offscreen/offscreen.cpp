// Original Caskrun native API control; no framework hooks or private reads.
#include <d3dmetal_native.h>
#include <dxgi1_4.h>
#include <d3d12.h>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <thread>

template<class T> struct Owned {
    T* p = nullptr;
    ~Owned() { if (p) p->Release(); }
    void** out() { return reinterpret_cast<void**>(&p); }
    T* operator->() const { return p; }
};
static bool checked(const char* stage, HRESULT hr) {
    std::printf("{\"stage\":\"%s\",\"hresult\":\"%08x\"}\n", stage, uint32_t(hr));
    std::fflush(stdout);
    return SUCCEEDED(hr);
}
extern "C" int caskrun_owned_offscreen_run() {
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    const auto initialized = dmn_init(nullptr);
    std::printf("{\"stage\":\"init\",\"result\":%d,\"initialized\":%s}\n",
                int(initialized), dmn_is_initialized() ? "true" : "false");
    if (initialized != DMN_SUCCESS || !dmn_is_initialized()) return 10;
    Owned<IDXGIFactory1> factory;
    if (!checked("factory", CreateDXGIFactory1(__uuidof(IDXGIFactory1), factory.out()))) return 11;
    Owned<IDXGIAdapter1> adapter;
    if (!checked("adapter", factory->EnumAdapters1(0, &adapter.p))) return 12;
    DXGI_ADAPTER_DESC1 description{};
    if (!checked("adapter-description", adapter->GetDesc1(&description))) return 13;
    if (description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) return 14;
    std::puts("{\"stage\":\"hardware-adapter\",\"result\":true}");
    Owned<ID3D12Device> device;
    if (!checked("device", D3D12CreateDevice(adapter.p, D3D_FEATURE_LEVEL_11_0,
                                             __uuidof(ID3D12Device), device.out()))) return 15;
    D3D12_COMMAND_QUEUE_DESC queueDescription{};
    queueDescription.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    Owned<ID3D12CommandQueue> queue;
    if (!checked("queue", device->CreateCommandQueue(&queueDescription,
                   __uuidof(ID3D12CommandQueue), queue.out()))) return 16;
    Owned<ID3D12CommandAllocator> allocator;
    if (!checked("allocator", device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,
                   __uuidof(ID3D12CommandAllocator), allocator.out()))) return 17;
    Owned<ID3D12GraphicsCommandList> commands;
    if (!checked("command-list", device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT,
                   allocator.p, nullptr, __uuidof(ID3D12GraphicsCommandList), commands.out()))) return 18;

    constexpr UINT width = 16, height = 16;
    const float color[] = {0.25f, 0.5f, 0.75f, 1.0f};
    D3D12_HEAP_PROPERTIES gpuHeap{};
    gpuHeap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC textureDescription{};
    textureDescription.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    textureDescription.Width = width;
    textureDescription.Height = height;
    textureDescription.DepthOrArraySize = 1;
    textureDescription.MipLevels = 1;
    textureDescription.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    textureDescription.SampleDesc.Count = 1;
    textureDescription.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    D3D12_CLEAR_VALUE optimizedClear{};
    optimizedClear.Format = textureDescription.Format;
    for (int i = 0; i < 4; ++i) optimizedClear.Color[i] = color[i];
    Owned<ID3D12Resource> texture;
    if (!checked("render-target", device->CreateCommittedResource(&gpuHeap, D3D12_HEAP_FLAG_NONE,
            &textureDescription, D3D12_RESOURCE_STATE_RENDER_TARGET, &optimizedClear,
            __uuidof(ID3D12Resource), texture.out()))) return 19;
    D3D12_DESCRIPTOR_HEAP_DESC descriptorDescription{};
    descriptorDescription.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    descriptorDescription.NumDescriptors = 1;
    Owned<ID3D12DescriptorHeap> descriptors;
    if (!checked("rtv-heap", device->CreateDescriptorHeap(&descriptorDescription,
                   __uuidof(ID3D12DescriptorHeap), descriptors.out()))) return 20;
    const auto targetHandle = descriptors->GetCPUDescriptorHandleForHeapStart();
    device->CreateRenderTargetView(texture.p, nullptr, targetHandle);
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
    UINT64 readbackBytes = 0;
    device->GetCopyableFootprints(&textureDescription, 0, 1, 0, &footprint, nullptr,
                                 nullptr, &readbackBytes);
    if (readbackBytes == 0 || readbackBytes > 1024 * 1024) return 21;
    constexpr UINT64 rowBytes = UINT64(width) * 4;
    if (footprint.Footprint.Format != textureDescription.Format ||
        footprint.Footprint.Width != width || footprint.Footprint.Height != height ||
        footprint.Footprint.Depth != 1 || footprint.Footprint.RowPitch < rowBytes ||
        footprint.Offset > readbackBytes) return 29;
    const UINT64 remaining = readbackBytes - footprint.Offset;
    if (remaining < rowBytes || UINT64(height - 1) * footprint.Footprint.RowPitch > remaining - rowBytes)
        return 29;
    D3D12_HEAP_PROPERTIES cpuHeap{};
    cpuHeap.Type = D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC bufferDescription{};
    bufferDescription.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    bufferDescription.Width = readbackBytes;
    bufferDescription.Height = 1;
    bufferDescription.DepthOrArraySize = 1;
    bufferDescription.MipLevels = 1;
    bufferDescription.SampleDesc.Count = 1;
    bufferDescription.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    Owned<ID3D12Resource> readback;
    if (!checked("readback-buffer", device->CreateCommittedResource(&cpuHeap,
            D3D12_HEAP_FLAG_NONE, &bufferDescription, D3D12_RESOURCE_STATE_COPY_DEST,
            nullptr, __uuidof(ID3D12Resource), readback.out()))) return 22;
    commands->ClearRenderTargetView(targetHandle, color, 0, nullptr);
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = texture.p;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    commands->ResourceBarrier(1, &barrier);
    D3D12_TEXTURE_COPY_LOCATION destination{};
    destination.pResource = readback.p;
    destination.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    destination.PlacedFootprint = footprint;
    D3D12_TEXTURE_COPY_LOCATION source{};
    source.pResource = texture.p;
    source.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    commands->CopyTextureRegion(&destination, 0, 0, 0, &source, nullptr);
    if (!checked("command-close", commands->Close())) return 23;
    Owned<ID3D12Fence> fence;
    if (!checked("fence", device->CreateFence(0, D3D12_FENCE_FLAG_NONE,
                   __uuidof(ID3D12Fence), fence.out()))) return 24;
    ID3D12CommandList* submission[] = {commands.p};
    queue->ExecuteCommandLists(1, submission);
    if (!checked("signal", queue->Signal(fence.p, 1))) return 25;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (fence->GetCompletedValue() < 1) {
        if (std::chrono::steady_clock::now() >= deadline) {
            std::puts("{\"stage\":\"fence-wait\",\"timeout\":true}");
            return 26;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    const D3D12_RANGE range{0, static_cast<SIZE_T>(readbackBytes)};
    void* mapped = nullptr;
    if (!checked("map", readback->Map(0, &range, &mapped))) return 27;
    if (!mapped) { const D3D12_RANGE written{0, 0}; readback->Unmap(0, &written); return 30; }
    const int expected[] = {64, 128, 191, 255};
    UINT good = 0;
    for (UINT y = 0; y < height; ++y) {
        const auto* row = static_cast<const uint8_t*>(mapped) + footprint.Offset +
                          y * footprint.Footprint.RowPitch;
        for (UINT x = 0; x < width; ++x) {
            bool match = true;
            for (int c = 0; c < 4; ++c) {
                int difference = int(row[4 * x + c]) - expected[c];
                if (difference < -1 || difference > 1) match = false;
            }
            good += match;
        }
    }
    const D3D12_RANGE written{0, 0};
    readback->Unmap(0, &written);
    std::printf("{\"stage\":\"pixel-check\",\"verified\":%u,\"total\":%u,\"passed\":%s}\n",
                good, width * height, good == width * height ? "true" : "false");
    return good == width * height ? 0 : 28;
}
