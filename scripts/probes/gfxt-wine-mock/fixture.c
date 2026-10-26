/* Original Caskrun own-prefix mock GFXT integration fixture. MIT. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "contract.h"

typedef LONG (WINAPI *prepare_fn)(void);
typedef LONG (WINAPI *run_fn)(struct caskrun_gfxt_probe_params*);

static int identity_gate(const char* base)
{
    char ready[32768], release[32768];
    HANDLE file;
    if (snprintf(ready, sizeof(ready), "%s.ready", base) >= (int)sizeof(ready) ||
        snprintf(release, sizeof(release), "%s.release", base) >= (int)sizeof(release)) return 0;
    if (GetFileAttributesA(release) != INVALID_FILE_ATTRIBUTES) return 0;
    file = CreateFileA(ready, GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return 0;
    CloseHandle(file);
    for (unsigned i = 0; i < 1500; ++i) {
        if (GetFileAttributesA(release) != INVALID_FILE_ATTRIBUTES) return 1;
        Sleep(20);
    }
    return 0;
}

int main(int argc, char** argv)
{
    static const wchar_t key_name[] = L"Software\\CaskrunGfxtOwnedProbe";
    const DWORD expected = 0x5a19b3u;
    HMODULE library = NULL;
    HKEY key = NULL;
    DWORD disposition = 0, type = 0, size = sizeof(DWORD), value = 0;
    int result = 1;
    const char* stage = "registry-create";
    if (argc != 2) return 2;
    if (RegCreateKeyExW(HKEY_CURRENT_USER, key_name, 0, NULL, 0, KEY_ALL_ACCESS,
                        NULL, &key, &disposition) != ERROR_SUCCESS ||
        disposition != REG_CREATED_NEW_KEY) goto done;
    stage = "registry-write";
    if (RegSetValueExW(key, L"probe", 0, REG_DWORD, (const BYTE*)&expected, sizeof(expected)) != ERROR_SUCCESS)
        goto done;
    stage = "dll-load";
    library = LoadLibraryW(L"caskrun_gfxt_probe.dll");
    if (!library) goto done;
    prepare_fn prepare = (prepare_fn)(void*)GetProcAddress(library, "CaskrunMockGFXTPrepare");
    run_fn run = (run_fn)(void*)GetProcAddress(library, "CaskrunMockGFXTRun");
    stage = "dll-exports";
    if (!prepare || !run) goto done;
    stage = "pe-prepare";
    if (prepare()) goto done;
    stage = "identity-gate";
    if (!identity_gate(argv[1])) goto done;
    struct caskrun_gfxt_probe_params request = {1, sizeof(request), (uint64_t)(uintptr_t)key, expected, 0, 0, 0};
    stage = "mock-call";
    if (run(&request) || request.checks != 7 || request.failures || request.native_pointers_returned) goto done;
    stage = "registry-read";
    if (RegQueryValueExW(key, L"probe", NULL, &type, (BYTE*)&value, &size) != ERROR_SUCCESS ||
        type != REG_DWORD || size != sizeof(value) || value != expected) goto done;
    printf("{\"fixture_passed\":true,\"actual_wine_registry\":true,\"checks\":%u,"
           "\"native_pointers_returned\":%u,\"apple_loaded\":false,\"game_compatibility_verified\":false}\n",
           request.checks, request.native_pointers_returned);
    result = 0;
done:
    if (library && !FreeLibrary(library)) { stage = "cleanup"; result = 1; }
    if (key) {
        if (RegCloseKey(key) != ERROR_SUCCESS) { stage = "cleanup"; result = 1; }
        if (disposition == REG_CREATED_NEW_KEY && RegDeleteKeyW(HKEY_CURRENT_USER, key_name) != ERROR_SUCCESS)
            { stage = "cleanup"; result = 1; }
    }
    if (result) fprintf(stderr, "CASKRUN_MOCK_STAGE:%s\n", stage);
    return result;
}
