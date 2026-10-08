/* Original generic Windows RtlGetVersion probe for Caskrun validation.
 * Build for Windows x64 and x86 using mingw-w64. Run the compiled executable
 * only in an owned disposable bottle, with the ordinary launcher harness.
 * --child <Windows path> also runs the companion x86 probe and waits for it.
 * A single marker-file argument creates that file and holds for 15 seconds,
 * allowing an idle-only setting refusal to be tested while this process lives.
 * Reports JSON to stdout; does not read or write any registry settings.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>
typedef LONG (WINAPI *VersionFn)(OSVERSIONINFOEXW *);
int main(int argc, char **argv) {
    OSVERSIONINFOEXW value; memset(&value, 0, sizeof(value)); value.dwOSVersionInfoSize=sizeof(value);
    HMODULE module=GetModuleHandleA("ntdll.dll"); FARPROC symbol=module?GetProcAddress(module,"RtlGetVersion"):NULL;
    VersionFn function=NULL; memcpy(&function,&symbol,sizeof(function));
    if(!function || function(&value)) return 2;
    #ifdef _WIN64
    const char *arch="x64";
#else
    const char *arch="x86";
#endif
    printf("{\"arch\":\"%s\",\"major\":%lu,\"minor\":%lu,\"build\":%lu,\"platform\":%lu}\n",arch,value.dwMajorVersion,value.dwMinorVersion,value.dwBuildNumber,value.dwPlatformId); fflush(stdout);
    if(argc==3 && strcmp(argv[1],"--child")==0) {
        STARTUPINFOA startup; PROCESS_INFORMATION child;
        memset(&startup,0,sizeof(startup)); memset(&child,0,sizeof(child)); startup.cb=sizeof(startup);
        if(!CreateProcessA(argv[2],NULL,NULL,NULL,TRUE,0,NULL,NULL,&startup,&child)) return 4;
        DWORD waited=WaitForSingleObject(child.hProcess,30000),code=1;
        if(waited!=WAIT_OBJECT_0 || !GetExitCodeProcess(child.hProcess,&code)) return 5;
        CloseHandle(child.hThread); CloseHandle(child.hProcess);
        if(code) return 6;
    }
    if(argc==2) {
        HANDLE ready=CreateFileA(argv[1],GENERIC_WRITE,FILE_SHARE_READ,NULL,CREATE_NEW,FILE_ATTRIBUTE_NORMAL,NULL);
        if(ready==INVALID_HANDLE_VALUE) return 3;
        CloseHandle(ready); Sleep(15000);
    }
    return 0;
}
