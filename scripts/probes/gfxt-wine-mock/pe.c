/* Original Caskrun mock-only Wine PE transport. MIT. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include "contract.h"

typedef LONG (WINAPI *query_fn)(HANDLE, void*, ULONG, void*, SIZE_T, SIZE_T*);
typedef LONG (WINAPI *dispatch_fn)(UINT64, unsigned, void*);
static UINT64 handle;
static dispatch_fn dispatch;

__declspec(dllexport) LONG WINAPI CaskrunMockGFXTPrepare(void)
{
    HMODULE self = GetModuleHandleW(L"caskrun_gfxt_probe.dll");
    HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
    query_fn query;
    dispatch_fn* dispatcher;
    if (!self || !ntdll) return (LONG)0xc0000001u;
    query = (query_fn)(void*)GetProcAddress(ntdll, "NtQueryVirtualMemory");
    dispatcher = (dispatch_fn*)(void*)GetProcAddress(ntdll, "__wine_unix_call_dispatcher");
    if (!query || !dispatcher || !*dispatcher) return (LONG)0xc0000001u;
    /* Wine's public MemoryWineUnixFuncs query; table belongs to this DLL. */
    if (query(GetCurrentProcess(), self, 1000, &handle, sizeof(handle), NULL) || !handle)
        return (LONG)0xc0000001u;
    dispatch = *dispatcher;
    return 0;
}

__declspec(dllexport) LONG WINAPI CaskrunMockGFXTRun(struct caskrun_gfxt_probe_params* request)
{
    if (!handle || !dispatch || !request) return (LONG)0xc000000du;
    return dispatch(handle, 0, request);
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, void* reserved)
{
    (void)instance; (void)reason; (void)reserved;
    return TRUE;
}
