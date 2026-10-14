/* SPDX-License-Identifier: MIT
 * Original Caskrun Windows fixture. No clipboard API is used: Ctrl+V writes a
 * fixed synthetic marker and records which field received it. Cut/Copy/Paste
 * messages are suppressed. All logs stay in this disposable prefix's drive C.
 * F2 enables ten seconds of selective key-only polling; mouse messages remain
 * pending so the native context action must refuse rather than edit field one.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>

static WNDPROC edit_proc;
static HWND window, fields[2], status;
static DWORD selective_until;
static HANDLE log_file = INVALID_HANDLE_VALUE;
static unsigned int sequence;

static void record(const char *event, int field)
{
    char line[256];
    DWORD written;
    int count = snprintf(line, sizeof(line),
        "{\"sequence\":%u,\"event\":\"%s\",\"field\":%d,\"control\":%d,\"shift\":%d,\"alt\":%d,\"win\":%d}\n",
        ++sequence, event, field, !!(GetKeyState(VK_CONTROL) & 0x8000),
        !!(GetKeyState(VK_SHIFT) & 0x8000), !!(GetKeyState(VK_MENU) & 0x8000),
        !!((GetKeyState(VK_LWIN) | GetKeyState(VK_RWIN)) & 0x8000));
    if (log_file != INVALID_HANDLE_VALUE && count > 0)
    {
        WriteFile(log_file, line, (DWORD)count, &written, NULL);
        FlushFileBuffers(log_file);
    }
}

static void selective_start(void)
{
    SetFocus(fields[0]);
    selective_until = GetTickCount() + 10000;
    SetCursor(LoadCursorW(NULL, IDC_IBEAM)); /* keep the observed text cursor during selective polling */
    SetWindowTextW(status, L"Key-only polling: native right-click Paste must refuse for ten seconds.");
    record("selective_begin", 1);
}

static LRESULT CALLBACK text_proc(HWND control, UINT message, WPARAM wparam, LPARAM lparam)
{
    int field = control == fields[0] ? 1 : 2;
    switch (message)
    {
    case WM_SETCURSOR:
        SetCursor(LoadCursorW(NULL, IDC_IBEAM));
        return TRUE;
    case WM_RBUTTONDOWN:
        SetFocus(control);
        record("right_down", field);
        return 0;
    case WM_RBUTTONUP:
        record("right_up", field);
        return 0; /* model Steam's text surface without a Windows popup */
    case WM_CONTEXTMENU:
    case WM_COPY:
    case WM_CUT:
    case WM_PASTE:
        return 0;
    case WM_KEYDOWN:
        if (wparam == VK_F2)
        {
            selective_start();
            return 0;
        }
        if ((wparam == 'C' || wparam == 'X') && (GetKeyState(VK_CONTROL) & 0x8000)) return 0;
        if (wparam == 'V' && (GetKeyState(VK_CONTROL) & 0x8000))
        {
            SetWindowTextW(control, L"Synthetic paste marker");
            SetWindowTextW(status, field == 1 ? L"Synthetic Paste reached first field." : L"Synthetic Paste reached second field.");
            record("synthetic_paste", field);
            return 0;
        }
        break;
    case WM_CHAR:
        if (wparam == 3 || wparam == 22 || wparam == 24) return 0;
        break;
    }
    return CallWindowProcW(edit_proc, control, message, wparam, lparam);
}

static LRESULT CALLBACK window_proc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam)
{
    switch (message)
    {
    case WM_COMMAND:
        if (LOWORD(wparam) == 10)
        {
            SetFocus(fields[0]);
            SetWindowTextW(fields[0], L"First field");
            SetWindowTextW(fields[1], L"Second field");
            record("focus_first", 1);
            return 0;
        }
        break;
    case WM_CLOSE:
        record("closed", 0);
        DestroyWindow(hwnd);
        return 0;
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hwnd, message, wparam, lparam);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, char *command_line, int show)
{
    WNDCLASSW klass = {0};
    MSG message;
    (void)previous; (void)command_line; (void)show;
    log_file = CreateFileW(L"C:\\native-text-fixture.jsonl", GENERIC_WRITE,
                          FILE_SHARE_READ, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    klass.lpfnWndProc = window_proc;
    klass.hInstance = instance;
    klass.hCursor = LoadCursorW(NULL, IDC_ARROW);
    klass.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);
    klass.lpszClassName = L"CaskrunNativeTextFixture";
    if (!RegisterClassW(&klass)) return 2;
    window = CreateWindowExW(0, klass.lpszClassName, L"Caskrun Native Paste Fixture",
                             WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT, 620, 290,
                             NULL, NULL, instance, NULL);
    if (!window) return 3;
    CreateWindowW(L"STATIC", L"No real clipboard is used. Native Paste inserts a fixed test marker.",
                  WS_CHILD | WS_VISIBLE, 24, 20, 560, 26, window, NULL, instance, NULL);
    fields[0] = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", L"First field",
                              WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL,
                              24, 66, 250, 38, window, (HMENU)1, instance, NULL);
    fields[1] = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", L"Second field",
                              WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL,
                              314, 66, 250, 38, window, (HMENU)2, instance, NULL);
    edit_proc = (WNDPROC)SetWindowLongPtrW(fields[0], GWLP_WNDPROC, (LONG_PTR)text_proc);
    SetWindowLongPtrW(fields[1], GWLP_WNDPROC, (LONG_PTR)text_proc);
    CreateWindowW(L"BUTTON", L"Focus first field", WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                  24, 126, 170, 32, window, (HMENU)10, instance, NULL);
    status = CreateWindowW(L"STATIC", L"Right-click second field and choose native Paste. F2 tests selective key polling.",
                            WS_CHILD | WS_VISIBLE, 24, 176, 560, 44, window, NULL, instance, NULL);
    ShowWindow(window, SW_SHOW);
    UpdateWindow(window);
    SetFocus(fields[0]);
    record("ready", 1);
    for (;;)
    {
        if (selective_until)
        {
            if ((LONG)(GetTickCount() - selective_until) >= 0)
            {
                selective_until = 0;
                record("selective_end", GetFocus() == fields[0] ? 1 : 2);
                SetWindowTextW(status, L"Normal polling restored.");
            }
            else if (!PeekMessageW(&message, NULL, WM_KEYFIRST, WM_KEYLAST, PM_REMOVE))
            {
                Sleep(5);
                continue;
            }
            else
            {
                TranslateMessage(&message);
                DispatchMessageW(&message);
                continue;
            }
        }
        if (!GetMessageW(&message, NULL, 0, 0)) break;
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    if (log_file != INVALID_HANDLE_VALUE) CloseHandle(log_file);
    return 0;
}
