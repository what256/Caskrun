/* SPDX-License-Identifier: MIT
 * Original Caskrun D3D12 hardware swap-chain lifecycle probe. No publisher content.
 * Build: x86_64-w64-mingw32-gcc -std=c11 -O2 -Wall -Wextra -Werror
 *        d3d12-present-smoke.c -o d3d12-present-smoke.exe -ladvapi32 -ldxguid -luser32
 *
 * Run only in a disposable prefix. Mandatory --identity-gate WINDOWS_PATH
 * writes a fresh .ready file after loading dxgi/d3d12. The host must prove
 * the mapped backend paths/hashes, then create .release. Logical DLL hashes
 * alone cannot identify Wine builtins' host implementation. No WARP fallback.
 * Required logical-file hashes: --expected-d3d12-sha256, --expected-dxgi-sha256.
 * A process watchdog bounds the complete probe to 30s; no store/game is run.
 * Passing proves bounded back-buffer clears/readback and successful Present,
 * resize and HWND recreation APIs. It does not prove game compatibility or
 * measure composited screen pixels. One nonactivating small window is visible
 * at a time; no account, game or publisher assets are used.
 */
#define COBJMACROS
#define WIDL_C_INLINE_WRAPPERS
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <wincrypt.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef HRESULT (WINAPI *CreateDeviceFn)(IUnknown *, D3D_FEATURE_LEVEL, REFIID, void **);
typedef HRESULT (WINAPI *CreateFactoryFn)(REFIID, void **);
static unsigned failures;
static const char *phase = "initialization";

static BOOL check(const char *name, BOOL passed, HRESULT hr)
{
    printf("{\"event\":\"check\",\"phase\":\"%s\",\"name\":\"%s\",\"passed\":%s,\"hresult\":\"0x%08lx\"}\n",
           phase, name, passed ? "true" : "false",
           (unsigned long)(uint32_t)(passed && FAILED(hr) ? S_OK : hr));
    fflush(stdout);
    if (!passed) ++failures;
    return passed;
}

static DWORD WINAPI watchdog(void *event)
{
    if (WaitForSingleObject(event, 30000) != WAIT_OBJECT_0)
        TerminateProcess(GetCurrentProcess(), 124);
    return 0;
}

static BOOL valid_hash(const char *hash)
{
    unsigned i;
    if (!hash || strlen(hash) != 64) return FALSE;
    for (i = 0; i < 64; ++i)
        if (!((hash[i] >= '0' && hash[i] <= '9') || (hash[i] >= 'a' && hash[i] <= 'f')))
            return FALSE;
    return TRUE;
}

static BOOL module_hash(HMODULE module, const char *expected)
{
    WCHAR path[32768];
    BYTE block[16384], digest[32];
    DWORD count, size = sizeof(digest), length;
    HCRYPTPROV provider = 0;
    HCRYPTHASH hash = 0;
    HANDLE file = INVALID_HANDLE_VALUE;
    uint64_t total = 0;
    char actual[65] = {0};
    BOOL ok = FALSE;
    length = GetModuleFileNameW(module, path, 32768);
    if (!length || length == 32768) goto done;
    file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE |
                       FILE_SHARE_DELETE, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) goto done;
    if (!CryptAcquireContextW(&provider, NULL, NULL, PROV_RSA_AES, CRYPT_VERIFYCONTEXT) ||
        !CryptCreateHash(provider, CALG_SHA_256, 0, 0, &hash)) goto done;
    for (;;) {
        if (!ReadFile(file, block, sizeof(block), &count, NULL)) goto done;
        if (!count) break;
        total += count;
        if (total > 128u * 1024u * 1024u || !CryptHashData(hash, block, count, 0)) goto done;
    }
    if (!CryptGetHashParam(hash, HP_HASHVAL, digest, &size, 0) || size != sizeof(digest)) goto done;
    for (count = 0; count < 32; ++count) snprintf(actual + count * 2, 3, "%02x", digest[count]);
    ok = strcmp(actual, expected) == 0;
done:
    if (hash) CryptDestroyHash(hash);
    if (provider) CryptReleaseContext(provider, 0);
    if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
    return ok;
}

static BOOL identity_gate(const char *argument)
{
    WCHAR ready[32768], release[32768];
    HANDLE file;
    int length = MultiByteToWideChar(CP_ACP, MB_ERR_INVALID_CHARS, argument, -1,
                                   ready, 32760);
    if (!length) return FALSE;
    memcpy(release, ready, (size_t)length * sizeof(*ready));
    memcpy(ready + length - 1, L".ready", sizeof(L".ready"));
    memcpy(release + length - 1, L".release", sizeof(L".release"));
    if (GetFileAttributesW(release) != INVALID_FILE_ATTRIBUTES) return FALSE;
    file = CreateFileW(ready, GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_NEW,
                       FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return FALSE;
    CloseHandle(file);
    printf("{\"event\":\"identity_ready\"}\n");
    fflush(stdout);
    while (GetFileAttributesW(release) == INVALID_FILE_ATTRIBUTES) Sleep(20);
    return TRUE;
}


typedef struct GPU {
    ID3D12Device *device;
    ID3D12CommandQueue *queue;
    ID3D12Fence *fence;
    HANDLE event;
    UINT64 serial;
} GPU;

static void pump_messages(void)
{
    MSG message;
    unsigned count = 0;
    while (count++ < 64 && PeekMessageW(&message, NULL, 0, 0, PM_REMOVE)) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}

static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_ERASEBKGND) return 1;
    return DefWindowProcW(window, message, wparam, lparam);
}

static BOOL wait_idle(GPU *gpu)
{
    HRESULT hr;
    UINT64 serial = ++gpu->serial;
    UINT64 completed;
    hr = ID3D12CommandQueue_Signal(gpu->queue, gpu->fence, serial);
    if (!check("queue_signal", hr == S_OK, hr)) return FALSE;
    hr = ID3D12Fence_SetEventOnCompletion(gpu->fence, serial, gpu->event);
    if (!check("fence_completion", hr == S_OK, hr)) return FALSE;
    if (!check("gpu_idle", WaitForSingleObject(gpu->event, 5000) == WAIT_OBJECT_0,
               E_FAIL)) return FALSE;
    completed = ID3D12Fence_GetCompletedValue(gpu->fence);
    return check("completed_fence_value", completed != UINT64_MAX && completed >= serial, E_FAIL);
}

static BOOL set_client_size(HWND window, UINT width, UINT height)
{
    RECT bounds = {0, 0, (LONG)width, (LONG)height}, actual = {0};
    DWORD style = (DWORD)GetWindowLongPtrW(window, GWL_STYLE);
    DWORD extended = (DWORD)GetWindowLongPtrW(window, GWL_EXSTYLE);
    ULONGLONG deadline;
    BOOL read = FALSE, matches = FALSE;
    if (!check("adjust_window_rect", AdjustWindowRectEx(&bounds, style, FALSE, extended),
               HRESULT_FROM_WIN32(GetLastError()))) return FALSE;
    if (!check("resize_hwnd", SetWindowPos(window, NULL, 0, 0,
               bounds.right - bounds.left, bounds.bottom - bounds.top,
               SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE),
               HRESULT_FROM_WIN32(GetLastError()))) return FALSE;
    /* Wine's native client view may apply the resize asynchronously. Keep
     * checking the actual client rectangle, with the same exact requirement. */
    deadline = GetTickCount64() + 1000;
    do {
        pump_messages();
        read = GetClientRect(window, &actual);
        matches = read && actual.right - actual.left == (LONG)width &&
                  actual.bottom - actual.top == (LONG)height;
        if (matches || GetTickCount64() >= deadline) break;
        Sleep(10);
    } while (TRUE);
    printf("{\"event\":\"client_dimensions\",\"phase\":\"%s\",\"rect_read\":%s,"
           "\"actual_width\":%ld,\"actual_height\":%ld,\"expected_width\":%u,\"expected_height\":%u}\n",
           phase, read ? "true" : "false", (long)(actual.right - actual.left),
           (long)(actual.bottom - actual.top), width, height);
    return check("client_size", matches, E_FAIL);
}

static HWND create_window(UINT width, UINT height)
{
    const DWORD style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    const DWORD extended = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
    RECT bounds = {0, 0, (LONG)width, (LONG)height};
    HWND window;
    if (!check("adjust_initial_rect", AdjustWindowRectEx(&bounds, style, FALSE, extended),
               HRESULT_FROM_WIN32(GetLastError()))) return NULL;
    window = CreateWindowExW(extended, L"CaskrunOriginalD3D12Lifecycle",
             L"Caskrun D3D12 Lifecycle", style, CW_USEDEFAULT, CW_USEDEFAULT,
             bounds.right - bounds.left, bounds.bottom - bounds.top, NULL, NULL,
             GetModuleHandleW(NULL), NULL);
    if (!check("create_hwnd", window != NULL, HRESULT_FROM_WIN32(GetLastError()))) return NULL;
    ShowWindow(window, SW_SHOWNOACTIVATE);
    UpdateWindow(window);
    pump_messages();
    if (!check("visible_hwnd", IsWindow(window) && IsWindowVisible(window), E_FAIL) ||
        !set_client_size(window, width, height)) {
        DestroyWindow(window);
        return NULL;
    }
    return window;
}

static BOOL validate_chain(IDXGISwapChain3 *chain, HWND window, UINT width, UINT height)
{
    DXGI_SWAP_CHAIN_DESC1 desc;
    HWND actual = NULL;
    HRESULT hr = IDXGISwapChain3_GetDesc1(chain, &desc);
    if (!check("swapchain_desc", hr == S_OK, hr)) return FALSE;
    if (!check("swapchain_size_format", desc.Width == width && desc.Height == height &&
               desc.Format == DXGI_FORMAT_R8G8B8A8_UNORM && desc.BufferCount == 2 &&
               desc.SampleDesc.Count == 1 && desc.SampleDesc.Quality == 0 &&
               desc.SwapEffect == DXGI_SWAP_EFFECT_FLIP_DISCARD &&
               (desc.BufferUsage & DXGI_USAGE_RENDER_TARGET_OUTPUT), E_FAIL)) return FALSE;
    hr = IDXGISwapChain3_GetHwnd(chain, &actual);
    return check("swapchain_hwnd", hr == S_OK && actual == window, hr);
}

static IDXGISwapChain3 *create_chain(GPU *gpu, IDXGIFactory2 *factory, HWND window,
                                     UINT width, UINT height)
{
    DXGI_SWAP_CHAIN_DESC1 desc = {0};
    IDXGISwapChain1 *first = NULL;
    IDXGISwapChain3 *chain = NULL;
    HRESULT hr;
    desc.Width = width; desc.Height = height;
    desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.BufferCount = 2;
    desc.Scaling = DXGI_SCALING_STRETCH;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    desc.AlphaMode = DXGI_ALPHA_MODE_IGNORE;
    hr = IDXGIFactory2_CreateSwapChainForHwnd(factory, (IUnknown *)gpu->queue,
         window, &desc, NULL, NULL, &first);
    if (!check("create_swapchain", hr == S_OK && first != NULL, hr)) goto done;
    hr = IDXGISwapChain1_QueryInterface(first, &IID_IDXGISwapChain3, (void **)&chain);
    if (!check("swapchain3", hr == S_OK && chain != NULL, hr) ||
        !validate_chain(chain, window, width, height)) {
        if (chain) { IDXGISwapChain3_Release(chain); chain = NULL; }
    }
done:
    if (first) IDXGISwapChain1_Release(first);
    return chain;
}

static BOOL frame(GPU *gpu, IDXGISwapChain3 *chain, UINT width, UINT height,
                  const float color[4], const BYTE expected[4])
{
    ID3D12CommandAllocator *allocator = NULL;
    ID3D12GraphicsCommandList *commands = NULL;
    ID3D12DescriptorHeap *heap = NULL;
    ID3D12Resource *texture = NULL, *readback = NULL;
    D3D12_DESCRIPTOR_HEAP_DESC descriptor = {0};
    D3D12_HEAP_PROPERTIES cpu = {0};
    D3D12_RESOURCE_DESC image, buffer = {0};
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint;
    D3D12_CPU_DESCRIPTOR_HANDLE rtv;
    D3D12_RESOURCE_BARRIER barrier = {0};
    D3D12_TEXTURE_COPY_LOCATION source = {0}, target = {0};
    D3D12_RANGE range;
    ID3D12CommandList *submission;
    UINT rows, x, y, index, next;
    UINT64 row_size, total;
    unsigned verified = 0;
    BYTE *pixels = NULL;
    HRESULT hr;
    BOOL ok = FALSE, gpu_dirty = FALSE;
#define CALL(label, expression) do { hr = (expression); if (!check(label, hr == S_OK, hr)) goto done; } while (0)
    pump_messages();
    index = IDXGISwapChain3_GetCurrentBackBufferIndex(chain);
    if (!check("backbuffer_index", index < 2, E_FAIL)) goto done;
    CALL("get_backbuffer", IDXGISwapChain3_GetBuffer(chain, index,
         &IID_ID3D12Resource, (void **)&texture));
    if (!check("backbuffer_nonnull", texture != NULL, E_FAIL)) goto done;
    image = ID3D12Resource_GetDesc(texture);
    if (!check("backbuffer_size_format", image.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D &&
               image.Width == width && image.Height == height &&
               image.Format == DXGI_FORMAT_R8G8B8A8_UNORM &&
               image.DepthOrArraySize == 1 && image.MipLevels == 1 &&
               image.SampleDesc.Count == 1, E_FAIL)) goto done;
    CALL("command_allocator", ID3D12Device_CreateCommandAllocator(gpu->device,
         D3D12_COMMAND_LIST_TYPE_DIRECT, &IID_ID3D12CommandAllocator, (void **)&allocator));
    CALL("command_list", ID3D12Device_CreateCommandList(gpu->device, 0,
         D3D12_COMMAND_LIST_TYPE_DIRECT, allocator, NULL,
         &IID_ID3D12GraphicsCommandList, (void **)&commands));
    descriptor.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    descriptor.NumDescriptors = 1;
    CALL("rtv_heap", ID3D12Device_CreateDescriptorHeap(gpu->device, &descriptor,
         &IID_ID3D12DescriptorHeap, (void **)&heap));
    ID3D12Device_GetCopyableFootprints(gpu->device, &image, 0, 1, 0, &footprint,
                                     &rows, &row_size, &total);
    if (!check("readback_layout", rows == height && row_size == (UINT64)width * 4 &&
               footprint.Footprint.RowPitch >= row_size && total > 0 && total <= 1048576 &&
               footprint.Offset + (height - 1) * footprint.Footprint.RowPitch + row_size <= total,
               E_FAIL)) goto done;
    cpu.Type = D3D12_HEAP_TYPE_READBACK;
    buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    buffer.Width = total; buffer.Height = 1;
    buffer.DepthOrArraySize = 1; buffer.MipLevels = 1;
    buffer.SampleDesc.Count = 1; buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    CALL("readback_buffer", ID3D12Device_CreateCommittedResource(gpu->device, &cpu,
         D3D12_HEAP_FLAG_NONE, &buffer, D3D12_RESOURCE_STATE_COPY_DEST, NULL,
         &IID_ID3D12Resource, (void **)&readback));
    rtv = ID3D12DescriptorHeap_GetCPUDescriptorHandleForHeapStart(heap);
    ID3D12Device_CreateRenderTargetView(gpu->device, texture, NULL, rtv);
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = texture;
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ID3D12GraphicsCommandList_ResourceBarrier(commands, 1, &barrier);
    ID3D12GraphicsCommandList_ClearRenderTargetView(commands, rtv, color, 0, NULL);
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    ID3D12GraphicsCommandList_ResourceBarrier(commands, 1, &barrier);
    source.pResource = texture;
    source.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    target.pResource = readback;
    target.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    target.PlacedFootprint = footprint;
    ID3D12GraphicsCommandList_CopyTextureRegion(commands, &target, 0, 0, 0, &source, NULL);
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    ID3D12GraphicsCommandList_ResourceBarrier(commands, 1, &barrier);
    CALL("close_commands", ID3D12GraphicsCommandList_Close(commands));
    submission = (ID3D12CommandList *)commands;
    ID3D12CommandQueue_ExecuteCommandLists(gpu->queue, 1, &submission);
    gpu_dirty = TRUE;
    if (!wait_idle(gpu)) goto done;
    gpu_dirty = FALSE;
    range.Begin = 0; range.End = (SIZE_T)total;
    CALL("map_readback", ID3D12Resource_Map(readback, 0, &range, (void **)&pixels));
    for (y = 0; y < height; ++y) for (x = 0; x < width; ++x) {
        unsigned c;
        BOOL match = TRUE;
        const BYTE *pixel = pixels + footprint.Offset + y * footprint.Footprint.RowPitch + x * 4;
        for (c = 0; c < 4; ++c) {
            int delta = (int)pixel[c] - expected[c];
            if (delta < -1 || delta > 1) match = FALSE;
        }
        if (match) ++verified;
    }
    range.Begin = 0; range.End = 0;
    ID3D12Resource_Unmap(readback, 0, &range);
    pixels = NULL;
    printf("{\"event\":\"backbuffer_pixels\",\"phase\":\"%s\",\"buffer\":%u,\"width\":%u,\"height\":%u,\"verified\":%u,\"total\":%u}\n",
           phase, index, width, height, verified, width * height);
    if (!check("backbuffer_pixels_match", verified == width * height, E_FAIL)) goto done;
    gpu_dirty = TRUE;
    CALL("present", IDXGISwapChain3_Present(chain, 0, 0));
    if (!wait_idle(gpu)) goto done;
    gpu_dirty = FALSE;
    next = IDXGISwapChain3_GetCurrentBackBufferIndex(chain);
    if (!check("backbuffer_rotated", next == (index + 1) % 2, E_FAIL)) goto done;
    pump_messages();
    ok = TRUE;
done:
    if (gpu_dirty) wait_idle(gpu);
    if (pixels) {
        range.Begin = 0; range.End = 0;
        ID3D12Resource_Unmap(readback, 0, &range);
    }
    if (commands) ID3D12GraphicsCommandList_Release(commands);
    if (allocator) ID3D12CommandAllocator_Release(allocator);
    if (heap) ID3D12DescriptorHeap_Release(heap);
    if (texture) ID3D12Resource_Release(texture);
    if (readback) ID3D12Resource_Release(readback);
    return ok;
#undef CALL
}

static BOOL lifecycle(ID3D12Device *device, IDXGIFactory2 *factory)
{
    GPU gpu = {0};
    WNDCLASSW window_class = {0};
    HWND window = NULL;
    IDXGISwapChain3 *chain = NULL;
    D3D12_COMMAND_QUEUE_DESC queue_desc = {0};
    const float initial_color[4] = {0.25f, 0.5f, 0.75f, 1.0f};
    const BYTE initial_bytes[4] = {64, 128, 191, 255};
    const float resized_color[4] = {0.75f, 0.25f, 0.5f, 1.0f};
    const BYTE resized_bytes[4] = {191, 64, 128, 255};
    const float recreated_color[4] = {0.5f, 0.75f, 0.25f, 1.0f};
    const BYTE recreated_bytes[4] = {128, 191, 64, 255};
    HRESULT hr;
    BOOL ok = FALSE, registered = FALSE;
    gpu.device = device;
#define CALL(label, expression) do { hr = (expression); if (!check(label, hr == S_OK, hr)) goto done; } while (0)
    queue_desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    CALL("command_queue", ID3D12Device_CreateCommandQueue(device, &queue_desc,
         &IID_ID3D12CommandQueue, (void **)&gpu.queue));
    CALL("fence", ID3D12Device_CreateFence(device, 0, D3D12_FENCE_FLAG_NONE,
         &IID_ID3D12Fence, (void **)&gpu.fence));
    gpu.event = CreateEventW(NULL, FALSE, FALSE, NULL);
    if (!check("fence_event", gpu.event != NULL, E_FAIL)) goto done;
    window_class.lpfnWndProc = window_proc;
    window_class.hInstance = GetModuleHandleW(NULL);
    window_class.hCursor = LoadCursorW(NULL, (LPCWSTR)IDC_ARROW);
    window_class.lpszClassName = L"CaskrunOriginalD3D12Lifecycle";
    if (!check("window_class", RegisterClassW(&window_class) != 0,
               HRESULT_FROM_WIN32(GetLastError()))) goto done;
    registered = TRUE;
    phase = "initial";
    window = create_window(240, 180);
    if (!window) goto done;
    chain = create_chain(&gpu, factory, window, 240, 180);
    if (!chain || !frame(&gpu, chain, 240, 180, initial_color, initial_bytes) ||
        !frame(&gpu, chain, 240, 180, initial_color, initial_bytes)) goto done;
    phase = "resized";
    /* frame releases every target, descriptor and list after GPU completion. */
    if (!wait_idle(&gpu) || !set_client_size(window, 320, 220)) goto done;
    CALL("resize_buffers", IDXGISwapChain3_ResizeBuffers(chain, 2, 320, 220,
         DXGI_FORMAT_R8G8B8A8_UNORM, 0));
    if (!validate_chain(chain, window, 320, 220) ||
        !frame(&gpu, chain, 320, 220, resized_color, resized_bytes) ||
        !frame(&gpu, chain, 320, 220, resized_color, resized_bytes)) goto done;
    if (!wait_idle(&gpu)) goto done;
    IDXGISwapChain3_Release(chain); chain = NULL;
    if (!check("destroy_first_hwnd", DestroyWindow(window),
               HRESULT_FROM_WIN32(GetLastError()))) goto done;
    if (!check("first_hwnd_gone", !IsWindow(window), E_FAIL)) goto done;
    window = NULL;
    pump_messages();
    phase = "recreated";
    window = create_window(288, 200);
    if (!window) goto done;
    chain = create_chain(&gpu, factory, window, 288, 200);
    if (!chain || !frame(&gpu, chain, 288, 200, recreated_color, recreated_bytes) ||
        !frame(&gpu, chain, 288, 200, recreated_color, recreated_bytes)) goto done;
    ok = TRUE;
done:
    if (gpu.queue && gpu.fence && gpu.event && !wait_idle(&gpu)) ok = FALSE;
    if (chain) IDXGISwapChain3_Release(chain);
    if (window && IsWindow(window)) {
        if (!check("destroy_remaining_hwnd", DestroyWindow(window),
                   HRESULT_FROM_WIN32(GetLastError()))) ok = FALSE;
    }
    pump_messages();
    if (registered && !check("unregister_window_class",
        UnregisterClassW(window_class.lpszClassName, window_class.hInstance),
        HRESULT_FROM_WIN32(GetLastError()))) ok = FALSE;
    if (gpu.event) CloseHandle(gpu.event);
    if (gpu.fence) ID3D12Fence_Release(gpu.fence);
    if (gpu.queue) ID3D12CommandQueue_Release(gpu.queue);
    return ok;
#undef CALL
}

int main(int argc, char **argv)
{
    const char *d3d_hash = NULL, *dxgi_hash = NULL, *gate = NULL;
    HMODULE dxgi = NULL, d3d = NULL;
    HANDLE done = NULL, thread = NULL;
    ID3D12Device *device = NULL;
    IDXGIFactory1 *factory = NULL;
    IDXGIFactory2 *factory2 = NULL;
    IDXGIAdapter1 *adapter = NULL;
    DXGI_ADAPTER_DESC1 adapter_desc;
    CreateDeviceFn create;
    CreateFactoryFn create_factory;
    HRESULT hr;
    int i, result = 2;
    for (i = 1; i < argc; ++i) {
        const char **value = NULL;
        if (!strcmp(argv[i], "--expected-d3d12-sha256")) value = &d3d_hash;
        else if (!strcmp(argv[i], "--expected-dxgi-sha256")) value = &dxgi_hash;
        else if (!strcmp(argv[i], "--identity-gate")) value = &gate;
        if (!value || *value || ++i == argc) return 2;
        *value = argv[i];
    }
    if (!valid_hash(d3d_hash) || !valid_hash(dxgi_hash) || !gate || !*gate) return 2;
    done = CreateEventW(NULL, TRUE, FALSE, NULL);
    if (!done) return 2;
    thread = CreateThread(NULL, 0, watchdog, done, 0, NULL);
    if (!thread) goto done;
    printf("{\"event\":\"probe\",\"schema\":1,\"api\":\"d3d12\",\"workload\":\"present_lifecycle\",\"fallback\":false}\n");
    fflush(stdout);
    dxgi = LoadLibraryW(L"dxgi.dll");
    if (!check("load_dxgi", dxgi != NULL, HRESULT_FROM_WIN32(GetLastError()))) goto done;
    d3d = LoadLibraryW(L"d3d12.dll");
    if (!check("load_d3d12", d3d != NULL, HRESULT_FROM_WIN32(GetLastError()))) goto done;
    if (!check("logical_dxgi_hash", module_hash(dxgi, dxgi_hash), E_FAIL) ||
        !check("logical_d3d12_hash", module_hash(d3d, d3d_hash), E_FAIL)) goto done;
    if (!check("host_identity_gate", identity_gate(gate), E_FAIL)) goto done;
    create_factory = (CreateFactoryFn)(void *)GetProcAddress(dxgi, "CreateDXGIFactory1");
    if (!check("factory_export", create_factory != NULL, E_FAIL)) goto done;
    hr = create_factory(&IID_IDXGIFactory1, (void **)&factory);
    if (!check("dxgi_factory", SUCCEEDED(hr) && factory != NULL, hr)) goto done;
    hr = IDXGIFactory1_QueryInterface(factory, &IID_IDXGIFactory2, (void **)&factory2);
    if (!check("factory2", hr == S_OK && factory2 != NULL, hr)) goto done;
    hr = IDXGIFactory1_EnumAdapters1(factory, 0, &adapter);
    if (!check("first_adapter", SUCCEEDED(hr) && adapter != NULL, hr)) goto done;
    hr = IDXGIAdapter1_GetDesc1(adapter, &adapter_desc);
    if (!check("hardware_adapter", SUCCEEDED(hr) &&
               !(adapter_desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE), hr)) goto done;
    create = (CreateDeviceFn)(void *)GetProcAddress(d3d, "D3D12CreateDevice");
    if (!check("device_export", create != NULL, E_FAIL)) goto done;
    hr = create((IUnknown *)adapter, D3D_FEATURE_LEVEL_11_0, &IID_ID3D12Device, (void **)&device);
    result = 1;
    if (!check("hardware_device_11_0", SUCCEEDED(hr) && device != NULL, hr)) goto done;
    if (lifecycle(device, factory2) && !failures) result = 0;
done:
    printf("{\"event\":\"result\",\"passed\":%s,\"game_compatibility_verified\":false,\"onscreen_pixels_verified\":false}\n",
           result == 0 ? "true" : "false");
    fflush(stdout);
    if (device) ID3D12Device_Release(device);
    if (adapter) IDXGIAdapter1_Release(adapter);
    if (factory2) IDXGIFactory2_Release(factory2);
    if (factory) IDXGIFactory1_Release(factory);
    if (d3d) FreeLibrary(d3d);
    if (dxgi) FreeLibrary(dxgi);
    SetEvent(done);
    if (thread) { WaitForSingleObject(thread, INFINITE); CloseHandle(thread); }
    CloseHandle(done);
    return result;
}
