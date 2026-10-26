/* SPDX-License-Identifier: MIT
 * Original optional D3DMetal host probe. Initialize a real, hidden Wine HWND
 * and HDC before loading graphics DLLs so the normal macOS user driver and its
 * PE callback dispatcher exist. The shared D3D12 probe still requires its
 * exact mapped-image identity gate and verifies hardware clear/readback.
 *
 * Build: x86_64-w64-mingw32-gcc -std=c11 -O2 -Wall -Wextra -Werror
 *        d3dmetal-smoke.c -o d3dmetal-smoke.exe -ladvapi32 -ldxguid -luser32 -lgdi32
 * Use only an owned prefix under a bounded external supervisor. The hidden
 * HWND is destroyed before exit; no game, store, user account or desktop HWND
 * is changed. This does not establish swapchain or game compatibility.
 */
#define main d3dmetal_clear_probe_main
#include "d3d12-smoke.c"
#undef main

int main(int argc, char **argv)
{
    HWND window;
    HDC dc;
    int result;

    window = CreateWindowExW(0, L"STATIC", L"Caskrun owned D3DMetal probe",
                             WS_OVERLAPPEDWINDOW, 0, 0, 64, 64, NULL, NULL,
                             GetModuleHandleW(NULL), NULL);
    if (!check("owned_hidden_window", window != NULL,
               HRESULT_FROM_WIN32(GetLastError()))) return 3;
    dc = GetDC(window);
    if (!check("owned_window_dc", dc != NULL, HRESULT_FROM_WIN32(GetLastError()))) {
        DestroyWindow(window);
        return 3;
    }
    ReleaseDC(window, dc);
    result = d3dmetal_clear_probe_main(argc, argv);
    if (!DestroyWindow(window) && !result) result = 3;
    return result;
}
