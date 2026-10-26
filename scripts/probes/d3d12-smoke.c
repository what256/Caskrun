/* SPDX-License-Identifier: MIT
 * Original Caskrun D3D12 hardware clear/readback probe. No publisher content.
 * Build: x86_64-w64-mingw32-gcc -std=c11 -O2 -Wall -Wextra -Werror
 *        d3d12-smoke.c -o d3d12-smoke.exe -ladvapi32 -ldxguid
 *
 * Run only in a disposable prefix. Mandatory --identity-gate WINDOWS_PATH
 * writes a fresh .ready file after loading dxgi/d3d12. The host must prove
 * the mapped backend paths/hashes, then create .release. Logical DLL hashes
 * alone cannot identify Wine builtins' host implementation. No WARP fallback.
 * Required logical-file hashes: --expected-d3d12-sha256, --expected-dxgi-sha256.
 * A process watchdog bounds the complete probe to 30s; no store/game is run.
 * Passing is an offscreen GPU clear/readback, not game or swap-chain validation.
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

static BOOL check(const char *name, BOOL passed, HRESULT hr)
{
    printf("{\"event\":\"check\",\"name\":\"%s\",\"passed\":%s,\"hresult\":\"0x%08lx\"}\n",
           name, passed ? "true" : "false", (unsigned long)(uint32_t)(passed ? S_OK : hr));
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

static BOOL render(ID3D12Device *device)
{
    ID3D12CommandQueue *queue = NULL;
    ID3D12CommandAllocator *allocator = NULL;
    ID3D12GraphicsCommandList *commands = NULL;
    ID3D12DescriptorHeap *heap = NULL;
    ID3D12Resource *texture = NULL, *readback = NULL;
    ID3D12Fence *fence = NULL;
    HANDLE event = NULL;
    D3D12_COMMAND_QUEUE_DESC queue_desc = {0};
    D3D12_DESCRIPTOR_HEAP_DESC descriptor = {0};
    D3D12_HEAP_PROPERTIES gpu = {0}, cpu = {0};
    D3D12_RESOURCE_DESC image = {0}, buffer = {0};
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint;
    D3D12_CPU_DESCRIPTOR_HANDLE rtv;
    D3D12_RESOURCE_BARRIER barrier = {0};
    D3D12_TEXTURE_COPY_LOCATION source = {0}, target = {0};
    D3D12_RANGE range;
    UINT rows, x, y;
    UINT64 row_size, total;
    const float color[4] = {0.25f, 0.5f, 0.75f, 1.0f};
    const BYTE expected[4] = {64, 128, 191, 255};
    BYTE *pixels = NULL;
    HRESULT hr;
    BOOL ok = FALSE;
#define CALL(label, expression) do { hr = (expression); if (!check(label, SUCCEEDED(hr), hr)) goto done; } while (0)
    queue_desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    CALL("command_queue", ID3D12Device_CreateCommandQueue(device, &queue_desc,
         &IID_ID3D12CommandQueue, (void **)&queue));
    CALL("command_allocator", ID3D12Device_CreateCommandAllocator(device,
         D3D12_COMMAND_LIST_TYPE_DIRECT, &IID_ID3D12CommandAllocator, (void **)&allocator));
    CALL("command_list", ID3D12Device_CreateCommandList(device, 0,
         D3D12_COMMAND_LIST_TYPE_DIRECT, allocator, NULL,
         &IID_ID3D12GraphicsCommandList, (void **)&commands));
    descriptor.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    descriptor.NumDescriptors = 1;
    CALL("rtv_heap", ID3D12Device_CreateDescriptorHeap(device, &descriptor,
         &IID_ID3D12DescriptorHeap, (void **)&heap));
    gpu.Type = D3D12_HEAP_TYPE_DEFAULT;
    image.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    image.Width = 16; image.Height = 16;
    image.DepthOrArraySize = 1; image.MipLevels = 1;
    image.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    image.SampleDesc.Count = 1;
    image.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    CALL("gpu_texture", ID3D12Device_CreateCommittedResource(device, &gpu,
         D3D12_HEAP_FLAG_NONE, &image, D3D12_RESOURCE_STATE_RENDER_TARGET, NULL,
         &IID_ID3D12Resource, (void **)&texture));
    ID3D12Device_GetCopyableFootprints(device, &image, 0, 1, 0, &footprint, &rows,
                                     &row_size, &total);
    if (!check("readback_layout", rows == 16 && row_size == 64 && total <= 65536,
               E_FAIL)) goto done;
    cpu.Type = D3D12_HEAP_TYPE_READBACK;
    buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    buffer.Width = total; buffer.Height = 1;
    buffer.DepthOrArraySize = 1; buffer.MipLevels = 1;
    buffer.SampleDesc.Count = 1; buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    CALL("readback_buffer", ID3D12Device_CreateCommittedResource(device, &cpu,
         D3D12_HEAP_FLAG_NONE, &buffer, D3D12_RESOURCE_STATE_COPY_DEST, NULL,
         &IID_ID3D12Resource, (void **)&readback));
    rtv = ID3D12DescriptorHeap_GetCPUDescriptorHandleForHeapStart(heap);
    ID3D12Device_CreateRenderTargetView(device, texture, NULL, rtv);
    ID3D12GraphicsCommandList_ClearRenderTargetView(commands, rtv, color, 0, NULL);
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = texture;
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ID3D12GraphicsCommandList_ResourceBarrier(commands, 1, &barrier);
    source.pResource = texture;
    source.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    target.pResource = readback;
    target.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    target.PlacedFootprint = footprint;
    ID3D12GraphicsCommandList_CopyTextureRegion(commands, &target, 0, 0, 0, &source, NULL);
    CALL("close_commands", ID3D12GraphicsCommandList_Close(commands));
    CALL("fence", ID3D12Device_CreateFence(device, 0, D3D12_FENCE_FLAG_NONE,
         &IID_ID3D12Fence, (void **)&fence));
    event = CreateEventW(NULL, FALSE, FALSE, NULL);
    if (!check("fence_event", event != NULL, E_FAIL)) goto done;
    ID3D12CommandQueue_ExecuteCommandLists(queue, 1, (ID3D12CommandList **)&commands);
    CALL("signal_fence", ID3D12CommandQueue_Signal(queue, fence, 1));
    CALL("fence_completion", ID3D12Fence_SetEventOnCompletion(fence, 1, event));
    if (!check("gpu_complete", WaitForSingleObject(event, 5000) == WAIT_OBJECT_0,
               E_FAIL)) goto done;
    range.Begin = 0; range.End = (SIZE_T)total;
    CALL("map_readback", ID3D12Resource_Map(readback, 0, &range, (void **)&pixels));
    ok = TRUE;
    for (y = 0; y < 16; ++y) for (x = 0; x < 16; ++x) {
        unsigned c;
        const BYTE *pixel = pixels + footprint.Offset + y * footprint.Footprint.RowPitch + x * 4;
        for (c = 0; c < 4; ++c) {
            int delta = (int)pixel[c] - expected[c];
            if (delta < -1 || delta > 1) ok = FALSE;
        }
    }
    range.Begin = 0; range.End = 0;
    ID3D12Resource_Unmap(readback, 0, &range);
    check("clear_pixels_match", ok, ok ? S_OK : E_FAIL);
done:
    if (event) CloseHandle(event);
    if (fence) ID3D12Fence_Release(fence);
    if (commands) ID3D12GraphicsCommandList_Release(commands);
    if (allocator) ID3D12CommandAllocator_Release(allocator);
    if (heap) ID3D12DescriptorHeap_Release(heap);
    if (texture) ID3D12Resource_Release(texture);
    if (readback) ID3D12Resource_Release(readback);
    if (queue) ID3D12CommandQueue_Release(queue);
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
    printf("{\"event\":\"probe\",\"schema\":1,\"api\":\"d3d12\",\"fallback\":false}\n");
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
    if (render(device) && !failures) result = 0;
done:
    printf("{\"event\":\"result\",\"passed\":%s,\"game_compatibility_verified\":false}\n",
           result == 0 ? "true" : "false");
    fflush(stdout);
    if (device) ID3D12Device_Release(device);
    if (adapter) IDXGIAdapter1_Release(adapter);
    if (factory) IDXGIFactory1_Release(factory);
    if (d3d) FreeLibrary(d3d);
    if (dxgi) FreeLibrary(dxgi);
    SetEvent(done);
    if (thread) { WaitForSingleObject(thread, INFINITE); CloseHandle(thread); }
    CloseHandle(done);
    return result;
}
