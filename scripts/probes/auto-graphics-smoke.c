/* SPDX-License-Identifier: MIT
 * Original Caskrun Auto-to-GPU smoke. No window, account or publisher content.
 * Build: x86_64-w64-mingw32-gcc -std=c11 -O2 -Wall -Wextra -Werror
 *   auto-graphics-smoke.c -o auto-graphics-smoke.exe -ld3d11
 * D3D11CreateDevice is deliberately a static import, so the real Auto scanner
 * can discover this executable without a fabricated graphics decision.
 */
#define COBJMACROS
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <d3d11.h>
#include <stdint.h>
#include <stdio.h>

static DWORD WINAPI watchdog(void *finished)
{
    if (WaitForSingleObject((HANDLE)finished, 30000) != WAIT_OBJECT_0)
        TerminateProcess(GetCurrentProcess(), 124); /* This probe process only. */
    return 0;
}

int main(void)
{
    ID3D11Device *device = NULL;
    ID3D11DeviceContext *context = NULL;
    ID3D11Texture2D *texture = NULL, *staging = NULL;
    ID3D11RenderTargetView *view = NULL;
    D3D_FEATURE_LEVEL chosen = 0;
    const D3D_FEATURE_LEVEL levels[] = {D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0};
    D3D11_TEXTURE2D_DESC description = {0};
    D3D11_MAPPED_SUBRESOURCE mapped = {0};
    const float colour[] = {32.0f / 255, 64.0f / 255, 128.0f / 255, 1.0f};
    const unsigned expected[] = {32, 64, 128, 255};
    unsigned rgba[4] = {0}, index;
    BOOL hardware = FALSE, passed = FALSE, metal = FALSE, wined3d = FALSE;
    HRESULT hr = E_FAIL;
    HANDLE finished = CreateEventW(NULL, TRUE, FALSE, NULL);
    HANDLE timer = finished ? CreateThread(NULL, 0, watchdog, finished, 0, NULL) : NULL;
    if (!timer) goto done;
    hr = D3D11CreateDevice(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, levels,
        sizeof(levels) / sizeof(*levels), D3D11_SDK_VERSION, &device, &chosen, &context);
    if (FAILED(hr)) goto done; /* No WARP or software fallback. */
    hardware = TRUE;
    description.Width = description.Height = description.MipLevels = description.ArraySize = 1;
    description.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_DEFAULT;
    description.BindFlags = D3D11_BIND_RENDER_TARGET;
    hr = ID3D11Device_CreateTexture2D(device, &description, NULL, &texture);
    if (FAILED(hr)) goto done;
    hr = ID3D11Device_CreateRenderTargetView(device, (ID3D11Resource *)texture, NULL, &view);
    if (FAILED(hr)) goto done;
    ID3D11DeviceContext_ClearRenderTargetView(context, view, colour);
    description.Usage = D3D11_USAGE_STAGING;
    description.BindFlags = 0;
    description.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    hr = ID3D11Device_CreateTexture2D(device, &description, NULL, &staging);
    if (FAILED(hr)) goto done;
    ID3D11DeviceContext_CopyResource(context, (ID3D11Resource *)staging, (ID3D11Resource *)texture);
    hr = ID3D11DeviceContext_Map(context, (ID3D11Resource *)staging, 0, D3D11_MAP_READ, 0, &mapped);
    if (FAILED(hr)) goto done;
    passed = mapped.pData != NULL && mapped.RowPitch >= 4;
    if (passed) {
        for (index = 0; index < 4; ++index) {
            rgba[index] = ((const unsigned char *)mapped.pData)[index];
            if (rgba[index] + 3 < expected[index] || rgba[index] > expected[index] + 3)
                passed = FALSE;
        }
    }
    ID3D11DeviceContext_Unmap(context, (ID3D11Resource *)staging, 0);
    metal = GetModuleHandleW(L"winemetal.dll") != NULL;
    wined3d = GetModuleHandleW(L"wined3d.dll") != NULL;
    passed = passed && metal && !wined3d;
done:
    if (view) ID3D11RenderTargetView_Release(view);
    if (staging) ID3D11Texture2D_Release(staging);
    if (texture) ID3D11Texture2D_Release(texture);
    if (context) ID3D11DeviceContext_Release(context);
    if (device) ID3D11Device_Release(device);
    printf("{\"schema\":1,\"architecture\":\"x64\",\"hardware\":%s,"
        "\"feature_level\":%u,\"rgba\":[%u,%u,%u,%u],\"winemetal_loaded\":%s,"
        "\"wined3d_loaded\":%s,\"hresult\":\"0x%08lx\",\"passed\":%s}\n",
        hardware ? "true" : "false", (unsigned)chosen, rgba[0], rgba[1], rgba[2], rgba[3],
        metal ? "true" : "false", wined3d ? "true" : "false",
        (unsigned long)(uint32_t)hr, passed ? "true" : "false");
    fflush(stdout);
    if (timer) { SetEvent(finished); WaitForSingleObject(timer, 1000); CloseHandle(timer); }
    if (finished) CloseHandle(finished);
    return passed ? 0 : 1;
}
