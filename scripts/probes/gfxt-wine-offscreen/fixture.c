/* Original Caskrun disposable-prefix offscreen control. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include "contract.h"
typedef LONG (WINAPI *prepare_fn)(void);
typedef LONG (WINAPI *run_fn)(struct caskrun_offscreen_request*);
static int gate(const char* base, const char* phase) {
    char ready[32768], release[32768];
    if (snprintf(ready, sizeof(ready), "%s.%s.ready", base, phase) >= (int)sizeof(ready) ||
        snprintf(release, sizeof(release), "%s.%s.release", base, phase) >= (int)sizeof(release)) return 0;
    if (GetFileAttributesA(release) != INVALID_FILE_ATTRIBUTES) return 0;
    HANDLE file = CreateFileA(ready, GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return 0;
    CloseHandle(file);
    for (unsigned i = 0; i < 1500; ++i) {
        if (GetFileAttributesA(release) != INVALID_FILE_ATTRIBUTES) return 1;
        Sleep(20);
    }
    return 0;
}
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    HMODULE library = LoadLibraryW(L"caskrun_gfxt_offscreen.dll");
    if (!library) return 3;
    prepare_fn prepare = (prepare_fn)(void*)GetProcAddress(library, "CaskrunOffscreenPrepare");
    run_fn run = (run_fn)(void*)GetProcAddress(library, "CaskrunOffscreenRun");
    int code = 4;
    if (!prepare || !run || prepare() || !gate(argv[1], "before")) goto done;
    struct caskrun_offscreen_request request = {1, sizeof(request), -1, 0};
    if (run(&request)) goto done;
    if (!gate(argv[1], "after")) goto done;
    printf("{\"result\":%ld,\"native_pointers_returned\":%lu}\n",
           (long)request.result, (unsigned long)request.native_pointers_returned);
    code = request.result == 0 && !request.native_pointers_returned ? 0 : 1;
done:
    FreeLibrary(library);
    return code;
}
