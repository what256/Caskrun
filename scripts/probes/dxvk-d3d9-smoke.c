/* SPDX-License-Identifier: MIT
 * Original Caskrun D3D9Ex hardware, fixed-function readback, Present and Reset probe.
 * Compile for x86 and x64 with MinGW: -std=c11 -O2 -Wall -Wextra -Werror
 *   -ladvapi32 -ldxguid -luser32
 * No game content or accounts. A required identity gate holds after loading
 * d3d9.dll. The caller must verify the live host mapping and its SHA-256,
 * then create GATE.release. GATE.ready announces the hold. A Wine builtin
 * module's logical filename alone cannot prove its actual runtime view.
 * No reference/software device fallback. Hard process deadline: 30 seconds.
 */
#define COBJMACROS
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <d3d9.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef HRESULT (WINAPI *Create9ExFn)(UINT, IDirect3D9Ex **);
static unsigned failures;
static BOOL record(const char *name, BOOL passed, HRESULT hr)
{
    printf("{\"event\":\"check\",\"name\":\"%s\",\"passed\":%s,\"hresult\":\"0x%08lx\"}\n",
        name, passed ? "true" : "false", (unsigned long)(uint32_t)(passed ? S_OK : hr));
    fflush(stdout);
    if (!passed) ++failures;
    return passed;
}
static DWORD WINAPI deadline(void *argument)
{
    if (WaitForSingleObject((HANDLE)argument, 30000) != WAIT_OBJECT_0)
        TerminateProcess(GetCurrentProcess(), 124);
    return 0;
}
static BOOL gate(const char *path)
{
    char ready[4096], release[4096];
    HANDLE file;
    if (strlen(path) > sizeof(ready) - 16) return FALSE;
    snprintf(ready, sizeof(ready), "%s.ready", path);
    snprintf(release, sizeof(release), "%s.release", path);
    if (GetFileAttributesA(release) != INVALID_FILE_ATTRIBUTES) return FALSE;
    file = CreateFileA(ready, GENERIC_WRITE, FILE_SHARE_READ, NULL,
        CREATE_NEW, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return FALSE;
    CloseHandle(file);
    while (GetFileAttributesA(release) == INVALID_FILE_ATTRIBUTES) Sleep(25);
    return TRUE;
}
static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    return DefWindowProcW(window, message, wparam, lparam);
}
static void pump(void)
{
    MSG message;
    while (PeekMessageW(&message, NULL, 0, 0, PM_REMOVE)) {
        TranslateMessage(&message); DispatchMessageW(&message);
    }
}
static BOOL pixel(IDirect3DDevice9Ex *device, unsigned width, unsigned height, BOOL triangle)
{
    IDirect3DSurface9 *target = NULL, *staging = NULL;
    D3DLOCKED_RECT locked;
    HRESULT hr;
    unsigned sample;
    BOOL ok = FALSE;
    hr = IDirect3DDevice9Ex_GetRenderTarget(device, 0, &target);
    if (!record("get_render_target", SUCCEEDED(hr), hr)) goto done;
    hr = IDirect3DDevice9Ex_CreateOffscreenPlainSurface(device, width, height,
        D3DFMT_A8R8G8B8, D3DPOOL_SYSTEMMEM, &staging, NULL);
    if (!record("create_staging_surface", SUCCEEDED(hr), hr)) goto done;
    hr = IDirect3DDevice9Ex_GetRenderTargetData(device, target, staging);
    if (!record("readback_surface", SUCCEEDED(hr), hr)) goto done;
    hr = IDirect3DSurface9_LockRect(staging, &locked, NULL, D3DLOCK_READONLY);
    if (!record("lock_staging_surface", SUCCEEDED(hr), hr)) goto done;
    ok = TRUE;
    for (sample = 0; sample < 2; ++sample) {
        unsigned x = sample ? width / 2 : 2, y = sample ? height / 2 : 2;
        const unsigned char *p = (const unsigned char *)locked.pBits + y * locked.Pitch + x * 4;
        unsigned r = p[2], g = p[1], b = p[0], a = p[3];
        unsigned er = triangle && sample ? 0 : 32, eg = triangle && sample ? 255 : 64;
        unsigned eb = triangle && sample ? 0 : 128;
        BOOL passed = r == er && g == eg && b == eb && a == 255;
        printf("{\"event\":\"pixel\",\"phase\":\"%s\",\"position\":\"%s\",\"passed\":%s,"
               "\"actual_rgba\":[%u,%u,%u,%u],\"expected_rgba\":[%u,%u,%u,255]}\n",
               triangle ? "triangle" : "clear", sample ? "center" : "corner",
               passed ? "true" : "false", r, g, b, a, er, eg, eb);
        if (!passed) { ++failures; ok = FALSE; }
    }
    IDirect3DSurface9_UnlockRect(staging);
done:
    if (staging) IDirect3DSurface9_Release(staging);
    if (target) IDirect3DSurface9_Release(target);
    return ok;
}
static BOOL render(IDirect3DDevice9Ex *device, unsigned width, unsigned height)
{
    struct Vertex { float x, y, z, rhw; DWORD color; } vertices[] = {
        {width * .15f, height * .85f, .5f, 1, 0xff00ff00},
        {width * .50f, height * .15f, .5f, 1, 0xff00ff00},
        {width * .85f, height * .85f, .5f, 1, 0xff00ff00}
    };
    D3DVIEWPORT9 viewport = {0, 0, width, height, 0, 1};
    HRESULT hr;
    hr = IDirect3DDevice9Ex_Clear(device, 0, NULL, D3DCLEAR_TARGET, 0xff204080, 1, 0);
    if (!record("clear", SUCCEEDED(hr), hr) || !pixel(device, width, height, FALSE)) return FALSE;
    hr = IDirect3DDevice9Ex_SetRenderState(device, D3DRS_CULLMODE, D3DCULL_NONE);
    if (!record("cull_state", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_SetRenderState(device, D3DRS_LIGHTING, FALSE);
    if (!record("lighting_state", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_SetRenderState(device, D3DRS_ZENABLE, FALSE);
    if (!record("depth_state", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_SetFVF(device, D3DFVF_XYZRHW | D3DFVF_DIFFUSE);
    if (!record("vertex_format", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_SetViewport(device, &viewport);
    if (!record("viewport", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_SetTexture(device, 0, NULL);
    if (!record("no_texture", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_SetTextureStageState(device, 0, D3DTSS_COLOROP, D3DTOP_SELECTARG1);
    if (!record("diffuse_color_op", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_SetTextureStageState(device, 0, D3DTSS_COLORARG1, D3DTA_DIFFUSE);
    if (!record("diffuse_color_arg", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_SetTextureStageState(device, 0, D3DTSS_ALPHAOP, D3DTOP_SELECTARG1);
    if (!record("diffuse_alpha_op", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_SetTextureStageState(device, 0, D3DTSS_ALPHAARG1, D3DTA_DIFFUSE);
    if (!record("diffuse_alpha_arg", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_SetTextureStageState(device, 1, D3DTSS_COLOROP, D3DTOP_DISABLE);
    if (!record("disable_second_stage", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_BeginScene(device);
    if (!record("begin_scene", SUCCEEDED(hr), hr)) return FALSE;
    hr = IDirect3DDevice9Ex_DrawPrimitiveUP(device, D3DPT_TRIANGLELIST, 1, vertices, sizeof(vertices[0]));
    if (!record("draw_triangle", SUCCEEDED(hr), hr)) { IDirect3DDevice9Ex_EndScene(device); return FALSE; }
    hr = IDirect3DDevice9Ex_EndScene(device);
    if (!record("end_scene", SUCCEEDED(hr), hr) || !pixel(device, width, height, TRUE)) return FALSE;
    hr = IDirect3DDevice9Ex_PresentEx(device, NULL, NULL, NULL, NULL, 0);
    pump();
    return record("present", SUCCEEDED(hr), hr);
}
int main(int argc, char **argv)
{
    HANDLE done = NULL, watchdog = NULL;
    HMODULE module = NULL;
    Create9ExFn create = NULL;
    IDirect3D9Ex *d3d = NULL;
    IDirect3DDevice9Ex *device = NULL;
    WNDCLASSW wc;
    HWND window = NULL;
    D3DPRESENT_PARAMETERS present;
    HRESULT hr;
    unsigned frame;
    if (argc != 3 || strcmp(argv[1], "--identity-gate")) return 2;
    setvbuf(stdout, NULL, _IONBF, 0);
    printf("{\"event\":\"probe\",\"schema\":1,\"api\":\"d3d9ex\",\"architecture\":\"%s\",\"watchdog_ms\":30000}\n",
        sizeof(void *) == 8 ? "x64" : "x86");
    done = CreateEventW(NULL, TRUE, FALSE, NULL);
    if (!record("watchdog_event", done != NULL, E_FAIL)) goto finish;
    watchdog = CreateThread(NULL, 0, deadline, done, 0, NULL);
    if (!record("watchdog_thread", watchdog != NULL, E_FAIL)) goto finish;
    module = LoadLibraryW(L"d3d9.dll");
    if (!record("load_d3d9", module != NULL, E_FAIL)) goto finish;
    if (!record("identity_gate", gate(argv[2]), E_FAIL)) goto finish;
    create = (Create9ExFn)(void *)GetProcAddress(module, "Direct3DCreate9Ex");
    if (!record("d3d9_export", create != NULL, E_FAIL)) goto finish;
    hr = create(D3D_SDK_VERSION, &d3d);
    if (!record("create_d3d9ex", SUCCEEDED(hr), hr)) goto finish;
    memset(&wc, 0, sizeof(wc)); wc.lpfnWndProc = window_proc;
    wc.hInstance = GetModuleHandleW(NULL); wc.lpszClassName = L"CaskrunDXVKD3D9Probe";
    if (!record("window_class", RegisterClassW(&wc) != 0, E_FAIL)) goto finish;
    window = CreateWindowW(wc.lpszClassName, L"Caskrun Direct3D 9 diagnostic",
        WS_OVERLAPPEDWINDOW | WS_VISIBLE, 100, 100, 280, 240, NULL, NULL, wc.hInstance, NULL);
    if (!record("create_hwnd", window != NULL, E_FAIL)) goto finish;
    memset(&present, 0, sizeof(present)); present.Windowed = TRUE;
    present.SwapEffect = D3DSWAPEFFECT_DISCARD; present.hDeviceWindow = window;
    present.BackBufferWidth = 128; present.BackBufferHeight = 128;
    present.BackBufferFormat = D3DFMT_A8R8G8B8; present.PresentationInterval = D3DPRESENT_INTERVAL_IMMEDIATE;
    hr = IDirect3D9Ex_CreateDeviceEx(d3d, D3DADAPTER_DEFAULT, D3DDEVTYPE_HAL, window,
        D3DCREATE_HARDWARE_VERTEXPROCESSING, &present, NULL, &device);
    if (!record("hardware_device", SUCCEEDED(hr), hr)) goto finish;
    for (frame = 0; frame < 4; ++frame) if (!render(device, 128, 128)) goto finish;
    present.BackBufferWidth = 192; present.BackBufferHeight = 96;
    hr = IDirect3DDevice9Ex_ResetEx(device, &present, NULL);
    if (!record("reset_resize", SUCCEEDED(hr), hr)) goto finish;
    for (frame = 0; frame < 2; ++frame) if (!render(device, 192, 96)) goto finish;
finish:
    if (device) IDirect3DDevice9Ex_Release(device);
    if (d3d) IDirect3D9Ex_Release(d3d);
    if (window) DestroyWindow(window);
    if (done) SetEvent(done);
    if (watchdog) { WaitForSingleObject(watchdog, 1000); CloseHandle(watchdog); }
    if (done) CloseHandle(done);
    if (module) FreeLibrary(module);
    printf("{\"event\":\"result\",\"passed\":%s,\"failed_checks\":%u}\n", failures ? "false" : "true", failures);
    return failures ? 1 : 0;
}
