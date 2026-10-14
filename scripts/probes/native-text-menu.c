/* SPDX-License-Identifier: MIT
 * Original read-only Steam menu metadata probe. No titles, text controls,
 * clipboard, account files or process arguments are read. Run only in an
 * authorized Caskrun prefix; write output only to ignored local diagnostics.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <wchar.h>

static BOOL helper_process(DWORD pid)
{
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    WCHAR image[4096], *base;
    DWORD length = sizeof(image) / sizeof(image[0]);
    BOOL helper = FALSE;
    if (!process) return FALSE;
    if (QueryFullProcessImageNameW(process, 0, image, &length))
    {
        base = wcsrchr(image, L'\\');
        base = base ? base + 1 : image;
        helper = !_wcsicmp(base, L"steamwebhelper.exe");
    }
    CloseHandle(process);
    return helper;
}

static BOOL CALLBACK snapshot(HWND hwnd, LPARAM opaque)
{
    DWORD pid, thread = GetWindowThreadProcessId(hwnd, &pid);
    GUITHREADINFO info = {0};
    WCHAR klass[128];
    HWND root = GetAncestor(hwnd, GA_ROOT);
    (void)opaque;
    if (!helper_process(pid) || !GetClassNameW(hwnd, klass, sizeof(klass) / sizeof(klass[0]))) return TRUE;
    info.cbSize = sizeof(info);
    if (!GetGUIThreadInfo(thread, &info)) return TRUE;
    printf("{\"popup_class\":%s,\"visible\":%s,\"foreground_root\":%s,\"menu_mode\":%s,"
           "\"menu_owner_same_root\":%s,\"menu_owner_owned_root\":%s,\"menu_owner_same_thread\":%s,\"capture\":%s}\n",
           !wcscmp(klass, L"#32768") ? "true" : "false", IsWindowVisible(hwnd) ? "true" : "false",
           root == GetAncestor(GetForegroundWindow(), GA_ROOT) ? "true" : "false",
           info.flags & GUI_INMENUMODE ? "true" : "false",
           info.hwndMenuOwner && GetAncestor(info.hwndMenuOwner, GA_ROOT) == root ? "true" : "false",
           info.hwndMenuOwner && GetAncestor(info.hwndMenuOwner, GA_ROOTOWNER) == root ? "true" : "false",
           info.hwndMenuOwner && GetWindowThreadProcessId(info.hwndMenuOwner, NULL) == thread ? "true" : "false",
           info.hwndCapture ? "true" : "false");
    return TRUE;
}

int main(void)
{
    EnumWindows(snapshot, 0);
    return 0;
}
