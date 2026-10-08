/* SPDX-License-Identifier: MIT
 * Original Caskrun Direct3D 11 probe. No game, account or publisher content.
 *
 * Build this same source with x86_64-w64-mingw32-gcc and i686-w64-mingw32-gcc:
 *   -std=c11 -O2 -Wall -Wextra -Werror dxmt-smoke.c -o probe.exe
 *   -ladvapi32 -ldxguid -luser32
 *
 * Run only in an owned disposable Wine prefix, with the DXMT DLLs installed
 * and appropriate overrides selected by the caller (builtin for Caskrun's
 * independently replaced builtin-DLL candidate):
 *   probe.exe --expected-d3d11-sha256 HEX --expected-dxgi-sha256 HEX
 * Add --lifecycle for schema 2: two concurrent windows, resize/readback,
 * recreate the primary swap chain on the same HWND, destroy that HWND before
 * swap-chain release, and render again in the surviving secondary window.
 * HEX must be the SHA-256 of that architecture's independently built DLL,
 * taken from the caller's build receipt. Matching names alone do not prove
 * DXMT selection. Module paths and hashes are local evidence, not source to
 * publish. The module records precede all rendering and a mismatch refuses
 * the test. A successful WineD3D run cannot be mistaken for a DXMT pass.
 *
 * Output is bounded JSON Lines, schema 1. Events: probe, module, check,
 * pixel, modules, result. Every check carries an exact feature_level (10_0
 * or 11_0) or "none". Hardware devices have no WARP/software fallback.
 * Both levels run an offscreen clear, an original HLSL triangle, staging
 * readback, then a real HWND swap chain, readback and four Present calls.
 * Readback checks known center/corner pixels, not merely API success.
 * A process watchdog exits 124 after 30 seconds, including a blocked driver.
 * Other exit codes: 0 all passed; 1 render failure; 2 arguments/identity.
 */
#define COBJMACROS
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <wincrypt.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define PROBE_SIZE 128u
#define HASH_LIMIT (128u * 1024u * 1024u)
#define WATCHDOG_MS 30000u

typedef HRESULT (WINAPI *CreateDeviceFn)(IDXGIAdapter *, D3D_DRIVER_TYPE,
    HMODULE, UINT, const D3D_FEATURE_LEVEL *, UINT, UINT, ID3D11Device **,
    D3D_FEATURE_LEVEL *, ID3D11DeviceContext **);
typedef HRESULT (WINAPI *CreateSwapFn)(IDXGIAdapter *, D3D_DRIVER_TYPE,
    HMODULE, UINT, const D3D_FEATURE_LEVEL *, UINT, UINT,
    const DXGI_SWAP_CHAIN_DESC *, IDXGISwapChain **, ID3D11Device **,
    D3D_FEATURE_LEVEL *, ID3D11DeviceContext **);
typedef HRESULT (WINAPI *CompileFn)(const void *, SIZE_T, const char *,
    const D3D_SHADER_MACRO *, ID3DInclude *, const char *, const char *, UINT,
    UINT, ID3DBlob **, ID3DBlob **);

static const char *level_name = "none";
static unsigned failures;
static BOOL window_closed;
static CreateDeviceFn create_device;
static CreateSwapFn create_swap;
static CompileFn compile_shader;

static void json_string(const char *value)
{
    const unsigned char *p = (const unsigned char *)value;
    putchar('"');
    for (; *p; ++p) {
        if (*p == '"' || *p == '\\') { putchar('\\'); putchar(*p); }
        else if (*p < 32) printf("\\u%04x", (unsigned)*p);
        else putchar(*p);
    }
    putchar('"');
}

static BOOL check(const char *name, BOOL passed, HRESULT hr)
{
    printf("{\"event\":\"check\",\"feature_level\":\"%s\",\"name\":", level_name);
    json_string(name);
    printf(",\"passed\":%s,\"hresult\":\"0x%08lx\"}\n",
        passed ? "true" : "false", (unsigned long)(uint32_t)(passed ? S_OK : hr));
    fflush(stdout);
    if (!passed) ++failures;
    return passed;
}

static DWORD WINAPI watchdog(void *argument)
{
    if (WaitForSingleObject((HANDLE)argument, WATCHDOG_MS) != WAIT_OBJECT_0) {
        /* No stream lock, pipe write or DLL detach may delay this deadline.
         * ExitProcess could wait for a loader lock held by the stuck driver.
         * The caller identifies this process-only deadline by exit code 124. */
        TerminateProcess(GetCurrentProcess(), 124);
    }
    return 0;
}

static BOOL valid_hash(const char *value)
{
    size_t i;
    if (!value || strlen(value) != 64) return FALSE;
    for (i = 0; i < 64; ++i)
        if (!((value[i] >= '0' && value[i] <= '9') ||
              (value[i] >= 'a' && value[i] <= 'f'))) return FALSE;
    return TRUE;
}

static BOOL module_identity(HMODULE module, const char *name, const char *expected)
{
    wchar_t path[32768];
    char utf8[32768 * 3] = {0}, actual[65] = {0};
    BYTE block[16384], digest[32];
    DWORD count, size = sizeof(digest), path_length;
    uint64_t total = 0;
    HCRYPTPROV provider = 0;
    HCRYPTHASH hash = 0;
    HANDLE file = INVALID_HANDLE_VALUE;
    BOOL ok = FALSE;
    path_length = GetModuleFileNameW(module, path, (DWORD)(sizeof(path) / sizeof(*path)));
    if (!path_length || path_length >= sizeof(path) / sizeof(*path)) goto done;
    if (!WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, path, -1,
        utf8, sizeof(utf8), NULL, NULL)) goto done;
    file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE |
        FILE_SHARE_DELETE, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) goto done;
    if (!CryptAcquireContextW(&provider, NULL, NULL, PROV_RSA_AES,
        CRYPT_VERIFYCONTEXT) || !CryptCreateHash(provider, CALG_SHA_256, 0, 0, &hash)) goto done;
    for (;;) {
        if (!ReadFile(file, block, sizeof(block), &count, NULL)) goto done;
        if (!count) break;
        total += count;
        if (total > HASH_LIMIT || !CryptHashData(hash, block, count, 0)) goto done;
    }
    if (!CryptGetHashParam(hash, HP_HASHVAL, digest, &size, 0) || size != sizeof(digest)) goto done;
    for (count = 0; count < sizeof(digest); ++count)
        snprintf(actual + count * 2, 3, "%02x", digest[count]);
    ok = strcmp(actual, expected) == 0;
done:
    printf("{\"event\":\"module\",\"name\":"); json_string(name);
    printf(",\"path\":"); json_string(utf8[0] ? utf8 : "unavailable");
    printf(",\"sha256\":"); json_string(actual);
    printf(",\"expected_sha256\":"); json_string(expected);
    printf(",\"matches_receipt\":%s}\n", ok ? "true" : "false");
    fflush(stdout);
    if (hash) CryptDestroyHash(hash);
    if (provider) CryptReleaseContext(provider, 0);
    if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
    return check(name, ok, ok ? S_OK : E_FAIL);
}

typedef struct Triangle {
    ID3D11VertexShader *vertex;
    ID3D11PixelShader *pixel;
    ID3D11InputLayout *layout;
    ID3D11Buffer *vertices;
    ID3D11RasterizerState *rasterizer;
} Triangle;

static void release_triangle(Triangle *triangle)
{
    if (triangle->rasterizer) ID3D11RasterizerState_Release(triangle->rasterizer);
    if (triangle->vertices) ID3D11Buffer_Release(triangle->vertices);
    if (triangle->layout) ID3D11InputLayout_Release(triangle->layout);
    if (triangle->pixel) ID3D11PixelShader_Release(triangle->pixel);
    if (triangle->vertex) ID3D11VertexShader_Release(triangle->vertex);
}

static BOOL make_triangle(ID3D11Device *device, Triangle *triangle)
{
    static const char shader[] =
        "float4 vertex_main(float2 p : POSITION) : SV_POSITION { return float4(p, 0.5, 1.0); }\n"
        "float4 pixel_main() : SV_TARGET { return float4(0.0, 1.0, 0.0, 1.0); }\n";
    static const float vertices[][2] = {{-0.75f, -0.75f}, {0.0f, 0.75f}, {0.75f, -0.75f}};
    D3D11_INPUT_ELEMENT_DESC input = {"POSITION", 0, DXGI_FORMAT_R32G32_FLOAT,
        0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0};
    D3D11_BUFFER_DESC buffer = {0};
    D3D11_SUBRESOURCE_DATA data = {0};
    D3D11_RASTERIZER_DESC rasterizer = {0};
    ID3DBlob *vertex = NULL, *pixel = NULL, *errors = NULL;
    HRESULT hr;
    BOOL ok = FALSE;
    hr = compile_shader(shader, sizeof(shader) - 1, "original-caskrun-triangle",
        NULL, NULL, "vertex_main", "vs_4_0", D3DCOMPILE_ENABLE_STRICTNESS, 0, &vertex, &errors);
    if (errors) { ID3D10Blob_Release(errors); errors = NULL; }
    if (!check("compile_vertex_shader", SUCCEEDED(hr) && vertex, hr)) goto done;
    hr = compile_shader(shader, sizeof(shader) - 1, "original-caskrun-triangle",
        NULL, NULL, "pixel_main", "ps_4_0", D3DCOMPILE_ENABLE_STRICTNESS, 0, &pixel, &errors);
    if (errors) { ID3D10Blob_Release(errors); errors = NULL; }
    if (!check("compile_pixel_shader", SUCCEEDED(hr) && pixel, hr)) goto done;
    hr = ID3D11Device_CreateVertexShader(device, ID3D10Blob_GetBufferPointer(vertex),
        ID3D10Blob_GetBufferSize(vertex), NULL, &triangle->vertex);
    if (!check("create_vertex_shader", SUCCEEDED(hr), hr)) goto done;
    hr = ID3D11Device_CreatePixelShader(device, ID3D10Blob_GetBufferPointer(pixel),
        ID3D10Blob_GetBufferSize(pixel), NULL, &triangle->pixel);
    if (!check("create_pixel_shader", SUCCEEDED(hr), hr)) goto done;
    hr = ID3D11Device_CreateInputLayout(device, &input, 1,
        ID3D10Blob_GetBufferPointer(vertex), ID3D10Blob_GetBufferSize(vertex), &triangle->layout);
    if (!check("create_input_layout", SUCCEEDED(hr), hr)) goto done;
    buffer.ByteWidth = sizeof(vertices);
    buffer.Usage = D3D11_USAGE_IMMUTABLE;
    buffer.BindFlags = D3D11_BIND_VERTEX_BUFFER;
    data.pSysMem = vertices;
    hr = ID3D11Device_CreateBuffer(device, &buffer, &data, &triangle->vertices);
    if (!check("create_vertex_buffer", SUCCEEDED(hr), hr)) goto done;
    rasterizer.FillMode = D3D11_FILL_SOLID;
    rasterizer.CullMode = D3D11_CULL_NONE;
    rasterizer.DepthClipEnable = TRUE;
    hr = ID3D11Device_CreateRasterizerState(device, &rasterizer, &triangle->rasterizer);
    ok = check("create_rasterizer", SUCCEEDED(hr), hr);
done:
    if (pixel) ID3D10Blob_Release(pixel);
    if (vertex) ID3D10Blob_Release(vertex);
    return ok;
}

static BOOL pixel_matches(const char *name, const BYTE *pixel,
    unsigned red, unsigned green, unsigned blue, unsigned alpha)
{
    const unsigned expected[] = {red, green, blue, alpha};
    unsigned i;
    BOOL ok = TRUE;
    for (i = 0; i < 4; ++i)
        if (abs((int)pixel[i] - (int)expected[i]) > 3) ok = FALSE;
    printf("{\"event\":\"pixel\",\"feature_level\":\"%s\",\"name\":", level_name);
    json_string(name);
    printf(",\"actual_rgba\":[%u,%u,%u,%u],\"expected_rgba\":[%u,%u,%u,%u],"
        "\"tolerance\":3,\"passed\":%s}\n", pixel[0], pixel[1], pixel[2], pixel[3],
        red, green, blue, alpha, ok ? "true" : "false");
    fflush(stdout);
    if (!ok) ++failures;
    return ok;
}

static BOOL read_pixels(ID3D11Device *device, ID3D11DeviceContext *context,
    ID3D11Texture2D *texture, const char *stage, BOOL triangle)
{
    D3D11_TEXTURE2D_DESC description;
    D3D11_MAPPED_SUBRESOURCE mapped;
    ID3D11Texture2D *staging = NULL;
    const BYTE *center, *corner;
    char name[80];
    HRESULT hr;
    BOOL ok = FALSE;
    ID3D11Texture2D_GetDesc(texture, &description);
    description.Usage = D3D11_USAGE_STAGING;
    description.BindFlags = 0;
    description.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    description.MiscFlags = 0;
    hr = ID3D11Device_CreateTexture2D(device, &description, NULL, &staging);
    if (!check("create_staging_texture", SUCCEEDED(hr), hr)) goto done;
    ID3D11DeviceContext_CopyResource(context, (ID3D11Resource *)staging, (ID3D11Resource *)texture);
    ID3D11DeviceContext_Flush(context);
    hr = ID3D11DeviceContext_Map(context, (ID3D11Resource *)staging, 0,
        D3D11_MAP_READ, 0, &mapped);
    if (!check("map_staging_texture", SUCCEEDED(hr), hr)) goto done;
    if (mapped.RowPitch < description.Width * 4 || !mapped.pData) {
        check("readback_layout", FALSE, E_FAIL);
        ID3D11DeviceContext_Unmap(context, (ID3D11Resource *)staging, 0);
        goto done;
    }
    corner = (const BYTE *)mapped.pData + 4 + mapped.RowPitch;
    center = (const BYTE *)mapped.pData + (description.Width / 2) * 4 +
        (description.Height / 2) * mapped.RowPitch;
    snprintf(name, sizeof(name), "%s_corner", stage);
    ok = pixel_matches(name, corner, 32, 64, 128, 255);
    snprintf(name, sizeof(name), "%s_center", stage);
    if (triangle) ok = pixel_matches(name, center, 0, 255, 0, 255) && ok;
    else ok = pixel_matches(name, center, 32, 64, 128, 255) && ok;
    ID3D11DeviceContext_Unmap(context, (ID3D11Resource *)staging, 0);
done:
    if (staging) ID3D11Texture2D_Release(staging);
    return ok;
}

static BOOL render(ID3D11Device *device, ID3D11DeviceContext *context,
    ID3D11Texture2D *texture, const char *stage)
{
    static const float clear[] = {0.125f, 0.25f, 0.5f, 1.0f};
    ID3D11RenderTargetView *view = NULL;
    Triangle triangle = {0};
    D3D11_TEXTURE2D_DESC description;
    D3D11_VIEWPORT viewport = {0};
    UINT stride = 2 * sizeof(float), offset = 0;
    char name[80];
    HRESULT hr;
    BOOL ok = FALSE;
    ID3D11Texture2D_GetDesc(texture, &description);
    hr = ID3D11Device_CreateRenderTargetView(device, (ID3D11Resource *)texture, NULL, &view);
    if (!check("create_render_target", SUCCEEDED(hr), hr)) goto done;
    ID3D11DeviceContext_ClearRenderTargetView(context, view, clear);
    snprintf(name, sizeof(name), "%s_clear", stage);
    if (!read_pixels(device, context, texture, name, FALSE)) goto done;
    if (!make_triangle(device, &triangle)) goto done;
    viewport.Width = (float)description.Width;
    viewport.Height = (float)description.Height;
    viewport.MaxDepth = 1.0f;
    ID3D11DeviceContext_OMSetRenderTargets(context, 1, &view, NULL);
    ID3D11DeviceContext_RSSetViewports(context, 1, &viewport);
    ID3D11DeviceContext_RSSetState(context, triangle.rasterizer);
    ID3D11DeviceContext_IASetInputLayout(context, triangle.layout);
    ID3D11DeviceContext_IASetVertexBuffers(context, 0, 1, &triangle.vertices, &stride, &offset);
    ID3D11DeviceContext_IASetPrimitiveTopology(context, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ID3D11DeviceContext_VSSetShader(context, triangle.vertex, NULL, 0);
    ID3D11DeviceContext_PSSetShader(context, triangle.pixel, NULL, 0);
    ID3D11DeviceContext_Draw(context, 3, 0);
    snprintf(name, sizeof(name), "%s_triangle", stage);
    ok = read_pixels(device, context, texture, name, TRUE);
done:
    ID3D11DeviceContext_ClearState(context);
    ID3D11DeviceContext_Flush(context);
    release_triangle(&triangle);
    if (view) ID3D11RenderTargetView_Release(view);
    return ok;
}

static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_CLOSE) { window_closed = TRUE; return 0; }
    return DefWindowProcW(window, message, wparam, lparam);
}

static BOOL pump_messages(void)
{
    MSG message;
    while (PeekMessageW(&message, NULL, 0, 0, PM_REMOVE)) {
        if (message.message == WM_QUIT) window_closed = TRUE;
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    return !window_closed;
}

static void run_level(HWND window, D3D_FEATURE_LEVEL level)
{
    D3D_FEATURE_LEVEL obtained = 0;
    ID3D11Device *device = NULL;
    ID3D11DeviceContext *context = NULL;
    ID3D11Texture2D *texture = NULL;
    IDXGISwapChain *swapchain = NULL;
    D3D11_TEXTURE2D_DESC texture_description = {0};
    DXGI_SWAP_CHAIN_DESC swap_description = {0};
    unsigned frame;
    HRESULT hr;
    level_name = level == D3D_FEATURE_LEVEL_10_0 ? "10_0" : "11_0";
    hr = create_device(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, &level, 1,
        D3D11_SDK_VERSION, &device, &obtained, &context);
    if (!check("hardware_device", SUCCEEDED(hr) && obtained == level && device && context, hr)) goto done;
    texture_description.Width = texture_description.Height = PROBE_SIZE;
    texture_description.MipLevels = texture_description.ArraySize = 1;
    texture_description.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    texture_description.SampleDesc.Count = 1;
    texture_description.Usage = D3D11_USAGE_DEFAULT;
    texture_description.BindFlags = D3D11_BIND_RENDER_TARGET;
    hr = ID3D11Device_CreateTexture2D(device, &texture_description, NULL, &texture);
    if (!check("offscreen_texture", SUCCEEDED(hr), hr)) goto done;
    render(device, context, texture, "offscreen");
    ID3D11Texture2D_Release(texture); texture = NULL;
    ID3D11DeviceContext_Release(context); context = NULL;
    ID3D11Device_Release(device); device = NULL;

    swap_description.BufferDesc.Width = swap_description.BufferDesc.Height = PROBE_SIZE;
    swap_description.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    swap_description.SampleDesc.Count = 1;
    swap_description.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    swap_description.BufferCount = 1;
    swap_description.OutputWindow = window;
    swap_description.Windowed = TRUE;
    swap_description.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
    hr = create_swap(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, &level, 1,
        D3D11_SDK_VERSION, &swap_description, &swapchain, &device, &obtained, &context);
    if (!check("hwnd_swapchain", SUCCEEDED(hr) && obtained == level &&
        swapchain && device && context, hr)) goto done;
    hr = IDXGISwapChain_GetBuffer(swapchain, 0, &IID_ID3D11Texture2D, (void **)&texture);
    if (!check("swapchain_backbuffer", SUCCEEDED(hr), hr)) goto done;
    for (frame = 0; frame < 4; ++frame) {
        if (!check("window_open", pump_messages(), S_OK)) break;
        if (!render(device, context, texture, "hwnd")) break;
        hr = IDXGISwapChain_Present(swapchain, 0, 0);
        /* DXGI_STATUS_OCCLUDED is a success code, but proves no visible present. */
        if (!check("present", hr == S_OK, hr)) break;
        Sleep(40);
    }
done:
    if (context) { ID3D11DeviceContext_ClearState(context); ID3D11DeviceContext_Flush(context); }
    if (texture) ID3D11Texture2D_Release(texture);
    if (swapchain) IDXGISwapChain_Release(swapchain);
    if (context) ID3D11DeviceContext_Release(context);
    if (device) ID3D11Device_Release(device);
}

typedef struct SwapTarget {
    HWND window;
    IDXGISwapChain *swapchain;
    ID3D11Device *device;
    ID3D11DeviceContext *context;
    ID3D11Texture2D *texture;
} SwapTarget;

static void release_swap_target(SwapTarget *target)
{
    if (target->context) {
        ID3D11DeviceContext_ClearState(target->context);
        ID3D11DeviceContext_Flush(target->context);
    }
    if (target->texture) ID3D11Texture2D_Release(target->texture);
    if (target->swapchain) IDXGISwapChain_Release(target->swapchain);
    if (target->context) ID3D11DeviceContext_Release(target->context);
    if (target->device) ID3D11Device_Release(target->device);
    target->texture = NULL;
    target->swapchain = NULL;
    target->context = NULL;
    target->device = NULL;
}

static BOOL lifecycle_create_window(SwapTarget *target, const wchar_t *title, const char *check_name)
{
    target->window = CreateWindowExW(0, L"CaskrunOriginalDXMTProbe", title,
        WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT, 384, 384, NULL, NULL,
        GetModuleHandleW(NULL), NULL);
    if (!check(check_name, target->window != NULL, HRESULT_FROM_WIN32(GetLastError()))) return FALSE;
    ShowWindow(target->window, SW_SHOWNORMAL);
    UpdateWindow(target->window);
    pump_messages();
    return TRUE;
}

static BOOL lifecycle_create_swap(SwapTarget *target, D3D_FEATURE_LEVEL level,
    UINT width, UINT height, const char *check_name)
{
    DXGI_SWAP_CHAIN_DESC description = {0};
    D3D_FEATURE_LEVEL obtained = 0;
    HRESULT hr;
    description.BufferDesc.Width = width;
    description.BufferDesc.Height = height;
    description.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    description.SampleDesc.Count = 1;
    description.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    description.BufferCount = 1;
    description.OutputWindow = target->window;
    description.Windowed = TRUE;
    description.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
    hr = create_swap(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, &level, 1,
        D3D11_SDK_VERSION, &description, &target->swapchain, &target->device,
        &obtained, &target->context);
    if (!check(check_name, SUCCEEDED(hr) && obtained == level &&
        target->swapchain && target->device && target->context, hr)) return FALSE;
    hr = IDXGISwapChain_GetBuffer(target->swapchain, 0, &IID_ID3D11Texture2D, (void **)&target->texture);
    return check("lifecycle_get_backbuffer", SUCCEEDED(hr), hr);
}

static BOOL lifecycle_render_present(SwapTarget *target, const char *stage, const char *present_name)
{
    HRESULT hr;
    if (!check("lifecycle_window_open", pump_messages() && IsWindow(target->window), S_OK)) return FALSE;
    if (!render(target->device, target->context, target->texture, stage)) return FALSE;
    hr = IDXGISwapChain_Present(target->swapchain, 0, 0);
    if (!check(present_name, hr == S_OK, hr)) return FALSE;
    Sleep(40);
    return TRUE;
}

static void run_lifecycle(D3D_FEATURE_LEVEL level)
{
    SwapTarget primary = {0}, secondary = {0};
    D3D11_TEXTURE2D_DESC resized;
    RECT rectangle = {0, 0, 192, 96};
    unsigned frame;
    HRESULT hr;
    level_name = level == D3D_FEATURE_LEVEL_10_0 ? "10_0" : "11_0";
    if (!lifecycle_create_window(&primary, L"Caskrun DXMT Lifecycle A", "lifecycle_primary_hwnd")) goto done;
    if (!lifecycle_create_window(&secondary, L"Caskrun DXMT Lifecycle B", "lifecycle_secondary_hwnd")) goto done;
    if (!lifecycle_create_swap(&primary, level, PROBE_SIZE, PROBE_SIZE, "lifecycle_primary_swapchain")) goto done;
    if (!lifecycle_create_swap(&secondary, level, PROBE_SIZE, PROBE_SIZE, "lifecycle_secondary_swapchain")) goto done;
    for (frame = 0; frame < 2; ++frame) {
        if (!lifecycle_render_present(&primary, "lifecycle_primary", "lifecycle_primary_present")) goto done;
        if (!lifecycle_render_present(&secondary, "lifecycle_secondary", "lifecycle_secondary_present")) goto done;
    }
    if (!check("lifecycle_resize_rectangle", AdjustWindowRect(&rectangle, WS_OVERLAPPEDWINDOW, FALSE),
        HRESULT_FROM_WIN32(GetLastError()))) goto done;
    if (!check("lifecycle_resize_hwnd", SetWindowPos(primary.window, NULL, 0, 0,
        rectangle.right - rectangle.left, rectangle.bottom - rectangle.top,
        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE), HRESULT_FROM_WIN32(GetLastError()))) goto done;
    pump_messages();
    ID3D11DeviceContext_ClearState(primary.context);
    ID3D11Texture2D_Release(primary.texture); primary.texture = NULL;
    hr = IDXGISwapChain_ResizeBuffers(primary.swapchain, 1, 192, 96, DXGI_FORMAT_R8G8B8A8_UNORM, 0);
    if (!check("lifecycle_resize_buffers", SUCCEEDED(hr), hr)) goto done;
    hr = IDXGISwapChain_GetBuffer(primary.swapchain, 0, &IID_ID3D11Texture2D, (void **)&primary.texture);
    if (!check("lifecycle_get_backbuffer", SUCCEEDED(hr), hr)) goto done;
    ID3D11Texture2D_GetDesc(primary.texture, &resized);
    printf("{\"event\":\"dimensions\",\"feature_level\":\"%s\",\"source\":\"resized_backbuffer\",\"width\":%u,\"height\":%u}\n",
        level_name, resized.Width, resized.Height);
    fflush(stdout);
    if (!check("lifecycle_resized_dimensions", resized.Width == 192 && resized.Height == 96, E_FAIL)) goto done;
    if (!lifecycle_render_present(&primary, "lifecycle_resized", "lifecycle_resized_present")) goto done;

    release_swap_target(&primary);
    check("lifecycle_release_before_recreate", TRUE, S_OK);
    if (!lifecycle_create_swap(&primary, level, 192, 96, "lifecycle_recreated_swapchain")) goto done;
    if (!lifecycle_render_present(&primary, "lifecycle_recreated", "lifecycle_recreated_present")) goto done;

    if (!check("lifecycle_destroy_primary_hwnd", DestroyWindow(primary.window), HRESULT_FROM_WIN32(GetLastError()))) goto done;
    primary.window = NULL;
    /* Do not call Present with an invalid HWND: that is a different contract.
     * Release must return safely while the native view's owner is gone. */
    release_swap_target(&primary);
    check("lifecycle_release_after_destroy", TRUE, S_OK);
    if (!lifecycle_render_present(&secondary, "lifecycle_survivor", "lifecycle_survivor_present")) goto done;
    release_swap_target(&secondary);
    check("lifecycle_release_secondary", TRUE, S_OK);
    if (!check("lifecycle_destroy_secondary_hwnd", DestroyWindow(secondary.window), HRESULT_FROM_WIN32(GetLastError()))) goto done;
    secondary.window = NULL;
done:
    release_swap_target(&primary);
    release_swap_target(&secondary);
    if (primary.window) DestroyWindow(primary.window);
    if (secondary.window) DestroyWindow(secondary.window);
}

int main(int argc, char **argv)
{
    const char *expected_d3d11 = NULL, *expected_dxgi = NULL;
    HMODULE d3d11 = NULL, dxgi = NULL, compiler = NULL;
    HANDLE completed = NULL, timer = NULL;
    WNDCLASSW window_class = {0};
    HWND window = NULL;
    BOOL identities_ok, lifecycle = FALSE;
    int i, result = 2;
    for (i = 1; i < argc; ++i) {
        if (!strcmp(argv[i], "--expected-d3d11-sha256") && !expected_d3d11 && i + 1 < argc)
            expected_d3d11 = argv[++i];
        else if (!strcmp(argv[i], "--expected-dxgi-sha256") && !expected_dxgi && i + 1 < argc)
            expected_dxgi = argv[++i];
        else if (!strcmp(argv[i], "--lifecycle") && !lifecycle)
            lifecycle = TRUE;
        else { fprintf(stderr, "Expected both --expected-d3d11-sha256 HEX and --expected-dxgi-sha256 HEX.\n"); return 2; }
    }
    if (!valid_hash(expected_d3d11) || !valid_hash(expected_dxgi)) {
        fprintf(stderr, "Both expected DLL SHA-256 values must be 64 lowercase hexadecimal characters.\n");
        return 2;
    }
    printf("{\"event\":\"probe\",\"schema\":%u,\"architecture\":\"%s\",\"watchdog_ms\":%u,\"driver\":\"hardware\"%s}\n",
        lifecycle ? 2u : 1u, sizeof(void *) == 8 ? "x64" : "x86", WATCHDOG_MS,
        lifecycle ? ",\"mode\":\"lifecycle\"" : "");
    fflush(stdout);
    completed = CreateEventW(NULL, TRUE, FALSE, NULL);
    if (!check("watchdog_event", completed != NULL, HRESULT_FROM_WIN32(GetLastError()))) goto done;
    timer = CreateThread(NULL, 0, watchdog, completed, 0, NULL);
    if (!check("watchdog_thread", timer != NULL, HRESULT_FROM_WIN32(GetLastError()))) goto done;
    d3d11 = LoadLibraryW(L"d3d11.dll");
    dxgi = LoadLibraryW(L"dxgi.dll");
    if (!check("load_d3d11_dxgi", d3d11 && dxgi, HRESULT_FROM_WIN32(GetLastError()))) goto done;
    identities_ok = module_identity(d3d11, "d3d11.dll", expected_d3d11);
    identities_ok = module_identity(dxgi, "dxgi.dll", expected_dxgi) && identities_ok;
    if (!identities_ok) goto done;
    /* memcpy avoids ISO C's object/function pointer cast diagnostic. Windows
     * guarantees GetProcAddress returns the ABI's function pointer value. */
    {
        FARPROC address = GetProcAddress(d3d11, "D3D11CreateDevice");
        memcpy(&create_device, &address, sizeof(create_device));
        address = GetProcAddress(d3d11, "D3D11CreateDeviceAndSwapChain");
        memcpy(&create_swap, &address, sizeof(create_swap));
    }
    if (!check("d3d11_exports", create_device && create_swap, E_NOINTERFACE)) goto done;
    compiler = LoadLibraryW(L"d3dcompiler_47.dll");
    if (!check("shader_compiler", compiler != NULL, HRESULT_FROM_WIN32(GetLastError()))) goto done;
    {
        FARPROC address = GetProcAddress(compiler, "D3DCompile");
        memcpy(&compile_shader, &address, sizeof(compile_shader));
    }
    if (!check("compiler_export", compile_shader != NULL, E_NOINTERFACE)) goto done;
    window_class.lpfnWndProc = window_proc;
    window_class.hInstance = GetModuleHandleW(NULL);
    window_class.lpszClassName = L"CaskrunOriginalDXMTProbe";
    if (!check("window_class", RegisterClassW(&window_class) != 0,
        HRESULT_FROM_WIN32(GetLastError()))) goto done;
    if (lifecycle) {
        run_lifecycle(D3D_FEATURE_LEVEL_10_0);
        run_lifecycle(D3D_FEATURE_LEVEL_11_0);
        result = failures ? 1 : 0;
        goto done;
    }
    window = CreateWindowExW(0, window_class.lpszClassName, L"Caskrun DXMT Smoke",
        WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT, 384, 384, NULL, NULL,
        window_class.hInstance, NULL);
    if (!check("create_hwnd", window != NULL, HRESULT_FROM_WIN32(GetLastError()))) goto done;
    ShowWindow(window, SW_SHOWNORMAL);
    UpdateWindow(window);
    pump_messages();
    run_level(window, D3D_FEATURE_LEVEL_10_0);
    run_level(window, D3D_FEATURE_LEVEL_11_0);
    result = failures ? 1 : 0;
done:
    printf("{\"event\":\"modules\",\"winemetal_loaded\":%s,\"wined3d_loaded\":%s}\n",
        GetModuleHandleW(L"winemetal.dll") ? "true" : "false",
        GetModuleHandleW(L"wined3d.dll") ? "true" : "false");
    if (window) DestroyWindow(window);
    if (compiler) FreeLibrary(compiler);
    if (dxgi) FreeLibrary(dxgi);
    if (d3d11) FreeLibrary(d3d11);
    printf("{\"event\":\"result\",\"passed\":%s,\"failed_checks\":%u,\"exit_code\":%d}\n",
        result == 0 ? "true" : "false", failures, result);
    fflush(stdout);
    if (completed) SetEvent(completed);
    if (timer) { WaitForSingleObject(timer, 1000); CloseHandle(timer); }
    if (completed) CloseHandle(completed);
    return result;
}
