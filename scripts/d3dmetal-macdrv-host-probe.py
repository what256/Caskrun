#!/usr/bin/env python3
"""Test the original Wine macdrv host contract with real owned Windows objects.

This fixture loads no Apple binary. A clearly named original mock implements
only the two published non-native-region interfaces. After PE main creates its
own HWND/HDC, loading original marker DLLs invokes the native fixture, which
calls the actual exported x64 driver table and real PE API callback dispatcher.
Both PE architectures are compiled; default execution validates x64 only.
--wow64-control requests a separate strict visible-window mapping control.
"""
from __future__ import annotations
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / ".build/d3dmetal-macdrv-integration"
TEST = WORK / "host-contract-test"
SRC = WORK / "src/wine-11.0"
BUILD = WORK / "base-build"
PACKAGE = ROOT / ".build/d3dmetal-bridge-integration/private-runtime"


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    sys.modules[name] = result
    spec.loader.exec_module(result)
    return result


NATIVE = r'''
#include "config.h"
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <dlfcn.h>
#include <dispatch/dispatch.h>
#include "macdrv.h"
#include "winreg.h"
#include "winerror.h"
#define D3DM_MSABI __attribute__((ms_abi))
#include "contract-layout.h"
static struct d3dmetal_macdrv_functions *api;
static struct d3dmetal_macdrv_window *snapshot;
static macdrv_metal_device device;
static unsigned checks, failures, phases;
static bool inside;
static int monitor_count;
static bool stop_monitor;
static TEB *(*current_teb)(void);
static BOOL D3DM_MSABI monitor_cb(HMONITOR monitor, HDC dc, LPRECT rect, LPARAM data)
{
    MONITORINFO info = {.cbSize=sizeof(info)};
    (void)dc; (void)rect;
    if (data != 0x12345) { fprintf(stderr,"monitor callback data mismatch: %lx\n",(unsigned long)data); return FALSE; }
    if (!api->monitor_info(monitor, &info)) { fprintf(stderr,"monitor info failed, size=%lu error=%lu\n",(unsigned long)sizeof(info),(unsigned long)current_teb()->LastErrorValue); return FALSE; }
    monitor_count++;
    if (stop_monitor) { current_teb()->LastErrorValue=0x76543210; return FALSE; }
    return TRUE;
}
#define CHECK(expr) do { checks++; if (!(expr)) { failures++; fprintf(stderr,"host fixture failed: %s\n",#expr); } } while (0)
static void run_first(HWND hwnd)
{
    HKEY key=NULL, opened=NULL;
    DWORD disposition=0, type=0, size=64;
    char value[64]={0};
    RECT rect={0,0,320,200};
    LONG_PTR previous;
    macdrv_metal_view view;
    __block int main_thread_ran=0;
    CHECK(sizeof(*api)==192 && sizeof(*snapshot)==120);
    CHECK(api->get_metric(SM_CXSCREEN)>0);
    current_teb()->LastErrorValue=0x12345678;
    CHECK(api->get_metric(SM_CXSCREEN)>0 && current_teb()->LastErrorValue==0x12345678);
    CHECK(api->reg_create(HKEY_CURRENT_USER,"Software\\CaskrunHostProbe",0,NULL,0,KEY_ALL_ACCESS,NULL,&key,&disposition)==0 && key && disposition==REG_CREATED_NEW_KEY);
    CHECK(api->reg_set(key,"sample",0,REG_SZ,(BYTE *)"original",9)==0);
    CHECK(api->reg_query(key,"sample",NULL,&type,(BYTE *)value,&size)==0 && type==REG_SZ && size==9 && !strcmp(value,"original"));
    CHECK(api->reg_open(HKEY_CURRENT_USER,"Software\\CaskrunHostProbe",0,KEY_READ,&opened)==0 && opened);
    CHECK(api->reg_close(opened)==0);
    CHECK(api->reg_close(key)==0);
    { BOOL ok=api->enum_monitors(NULL,NULL,monitor_cb,0x12345);
      fprintf(stderr,"native monitor enumeration: ok=%d count=%d error=%lu\n",ok,monitor_count,(unsigned long)current_teb()->LastErrorValue);
      CHECK(ok && monitor_count>0); }
    stop_monitor=true; monitor_count=0;
    CHECK(!api->enum_monitors(NULL,NULL,monitor_cb,0x12345) && monitor_count==1 && current_teb()->LastErrorValue==0x76543210);
    stop_monitor=false; monitor_count=0;
    { RECT empty={0,0,0,0}; CHECK(api->enum_monitors(NULL,&empty,monitor_cb,0x12345) && monitor_count==0); }
    api->init_displays(TRUE); monitor_count=0;
    CHECK(api->enum_monitors(NULL,NULL,monitor_cb,0x12345) && monitor_count>0);
    CHECK(api->adjust_rect(&rect,WS_OVERLAPPEDWINDOW,FALSE,0) && rect.right-rect.left>=320 && rect.bottom-rect.top>=200);
    CHECK(api->get_window_long(hwnd,GWL_STYLE)&WS_OVERLAPPEDWINDOW);
    CHECK(api->window_rect(hwnd,&rect) && rect.right>rect.left && rect.bottom>rect.top);
    current_teb()->LastErrorValue=0;
    CHECK(!api->window_rect((HWND)(UINT_PTR)0x12345678,&rect) && current_teb()->LastErrorValue==ERROR_INVALID_WINDOW_HANDLE);
    CHECK(api->move_window(hwnd,40,50,420,260,FALSE));
    CHECK(api->set_window_pos(hwnd,NULL,50,60,430,270,SWP_NOZORDER|SWP_NOACTIVATE));
    previous=api->set_window_long(hwnd,GWLP_USERDATA,0x12345678);
    CHECK(api->get_window_long(hwnd,GWLP_USERDATA)==0x12345678);
    CHECK(api->set_window_long(hwnd,GWLP_USERDATA,previous)==0x12345678);
    api->init_displays(FALSE);
    CHECK(api->get_cocoa_window(hwnd,FALSE)!=NULL);
    api->main_thread(^{ main_thread_ran=1; });
    CHECK(main_thread_ran==1);
    CHECK(api->get_window(NULL)==NULL);
    CHECK(api->get_window((HWND)(UINT_PTR)0x12345678)==NULL);
    api->release_window((void *)1);
    api->release_view((void *)1);
    CHECK(api->get_layer((void *)1)==NULL);
    device=api->create_device();
    CHECK(device!=NULL);
    if (!device) return;
    snapshot=api->get_window(hwnd);
    CHECK(snapshot && snapshot->hwnd==hwnd && snapshot->cocoa_window && snapshot->cocoa_view && snapshot->client_cocoa_view);
    if (!snapshot) return;
    CHECK(api->create_view((void *)1,device)==NULL);
    view=api->create_view(snapshot->client_cocoa_view,device);
    CHECK(view!=NULL);
    if (view) {
        CHECK(api->get_layer(view)!=NULL);
        api->release_view(view);
        api->release_view(view);
        CHECK(api->get_layer(view)==NULL);
    }
    view=api->create_view(snapshot->client_cocoa_view,device);
    CHECK(view!=NULL);
    if (view) {
        CHECK(api->get_layer(view)!=NULL);
        api->release_view(view);
    }
}
static void run_destroyed(HWND hwnd)
{
    CHECK(api->get_window(hwnd)==NULL);
    if (snapshot && device) CHECK(api->create_view(snapshot->client_cocoa_view,device)==NULL);
    if (snapshot) {
        api->release_window(snapshot);
        api->release_window(snapshot);
        snapshot=NULL;
    }
    if (device) { api->release_device(device); device=NULL; }
}
bool supports_non_native_code_regions(void) { return true; }
void register_non_native_code_region(void *start, void *end)
{
    const char *state_path=getenv("CASKRUN_HOST_TEST_STATE");
    const char *result_path=getenv("CASKRUN_HOST_TEST_RESULT");
    const char *driver_path=getenv("CASKRUN_HOST_TEST_DRIVER");
    FILE *state, *result;
    unsigned phase;
    unsigned long long hwnd;
    void *driver;
    (void)start; (void)end;
    if (inside || !state_path || !(state=fopen(state_path,"r"))) return;
    if (fscanf(state,"%u %llx",&phase,&hwnd)!=2) { fclose(state); return; }
    fclose(state);
    if (phase!=phases+1 || phase>2) return;
    inside=true;
    if (!api) {
        driver=driver_path ? dlopen(driver_path,RTLD_NOW|RTLD_LOCAL|RTLD_NOLOAD) : NULL;
        api=driver ? dlsym(driver,"macdrv_functions") : NULL;
        CHECK(api!=NULL);
        current_teb=dlsym(RTLD_DEFAULT,"NtCurrentTeb");
        CHECK(current_teb!=NULL);
    }
    if (api && current_teb && phase==1) run_first((HWND)(UINT_PTR)hwnd);
    if (api && phase==2) run_destroyed((HWND)(UINT_PTR)hwnd);
    phases=phase;
    result=result_path ? fopen(result_path,"w") : NULL;
    if (result) {
        fprintf(result,"{\"passed\":%s,\"checks\":%u,\"failures\":%u,\"phases\":%u,\"host_slots\":24,\"pe_callback_slots\":14,\"apple_payload\":false}\n",!failures&&phases==2?"true":"false",checks,failures,phases);
        fclose(result);
    }
    inside=false;
}
'''

PE = r'''
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
static unsigned baseline_count;
static BOOL CALLBACK baseline_monitor(HMONITOR m,HDC d,LPRECT r,LPARAM p) {(void)m;(void)d;(void)r;(void)p;baseline_count++;return TRUE;}
static void state(const char *path,unsigned phase,HWND window) {
    FILE *f=fopen(path,"w"); if(!f) exit(10); fprintf(f,"%u %llx\n",phase,(unsigned long long)(uintptr_t)window); fclose(f);
}
int main(int argc,char **argv) {
    WNDCLASSA cls={0}; HWND window; HDC dc; HMODULE marker; FILE *f; unsigned wait;
    if(argc!=7) return 1;
    cls.lpfnWndProc=DefWindowProcA; cls.hInstance=GetModuleHandleA(NULL); cls.lpszClassName="CaskrunOriginalHostFixture";
    if(!RegisterClassA(&cls)) return 2;
    window=CreateWindowExA(0,cls.lpszClassName,"Caskrun host contract fixture",WS_OVERLAPPEDWINDOW,0,0,320,200,NULL,NULL,cls.hInstance,NULL);
    if(!window || !(dc=GetDC(window))) return 3;
    { BOOL ok=EnumDisplayMonitors(NULL,NULL,baseline_monitor,0); fprintf(stderr,"PE baseline monitor enumeration: ok=%d count=%u error=%lu\n",ok,baseline_count,(unsigned long)GetLastError()); }
    f=fopen(argv[4],"wx"); if(!f) return 4; fputs("ready",f); fclose(f);
    for(wait=0;wait<300 && GetFileAttributesA(argv[5])==INVALID_FILE_ATTRIBUTES;wait++) Sleep(50);
    if(wait==300) return 5;
    state(argv[1],1,window); marker=LoadLibraryA(argv[2]); if(!marker) return 6; FreeLibrary(marker);
    ReleaseDC(window,dc); if(!DestroyWindow(window)) return 7;
    state(argv[1],2,window); marker=LoadLibraryA(argv[3]); if(!marker) return 8; FreeLibrary(marker);
    UnregisterClassA(cls.lpszClassName,cls.hInstance);
    f=fopen(argv[6],"r"); if(!f) return 9; fclose(f);
    puts("{\"fixture_main_completed\":true}"); return 0;
}
'''


PE32 = r'''
#include <windows.h>
#include <stdio.h>
int main(int argc,char **argv) {
    WNDCLASSA cls={0}; HWND window; HDC dc; RECT rect; FILE *f; unsigned wait;
    if(argc!=3) return 1;
    cls.lpfnWndProc=DefWindowProcA;cls.hInstance=GetModuleHandleA(NULL);cls.lpszClassName="CaskrunOriginalHostWow64";
    if(!RegisterClassA(&cls)) return 2;
    window=CreateWindowExA(0,cls.lpszClassName,"Caskrun owned WoW64 host control",WS_OVERLAPPEDWINDOW,0,0,320,200,NULL,NULL,cls.hInstance,NULL);
    if(!window) return 3;
    ShowWindow(window,SW_SHOWNOACTIVATE);UpdateWindow(window);
    if(!(dc=GetDC(window))) return 3;
    f=fopen(argv[1],"wx");if(!f)return 4;fputs("ready",f);fclose(f);
    for(wait=0;wait<300 && GetFileAttributesA(argv[2])==INVALID_FILE_ATTRIBUTES;wait++)Sleep(50);
    if(wait==300)return 5;
    if(!GetWindowRect(window,&rect) || rect.right<=rect.left || rect.bottom<=rect.top)return 6;
    ReleaseDC(window,dc);if(!DestroyWindow(window))return 7;
    if(!UnregisterClassA(cls.lpszClassName,cls.hInstance))return 8;
    puts("{\"wow64_window_lifecycle\":true}");return 0;
}
'''


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wow64-control",action="store_true",help="also require a separate shown WoW64 window's exact mapped driver (not accepted by the current x64-only fixture evidence)")
    arguments=parser.parse_args()
    runner=module("d3dmetal_host_owned_session",ROOT/"scripts/test-dxmt-d3d12-dev.py")
    builder=module("d3dmetal_host_source_builder",ROOT/"scripts/build-wine-steam-reopen-dev.py")
    TEST.mkdir(exist_ok=True)
    source=(SRC/"dlls/winemac.drv/d3dmetal-macdrv-host.c").read_text()
    window=re.search(r"struct d3dmetal_macdrv_window\n\{.*?\n\};",source,re.S).group()
    table=re.search(r"struct d3dmetal_macdrv_functions\n\{.*?\n\};",source,re.S).group()
    monitor="typedef BOOL (D3DM_MSABI *host_monitor_callback)(HMONITOR,HDC,LPRECT,LPARAM);\n"
    (TEST/"contract-layout.h").write_text(window+"\n"+monitor+table+"\n")
    (TEST/"mock-host-support.c").write_text(NATIVE)
    (TEST/"main.c").write_text(PE)
    (TEST/"wow64.c").write_text(PE32)
    (TEST/"marker.c").write_text("#include <windows.h>\nBOOL WINAPI DllMain(HINSTANCE h,DWORD r,void*p){(void)h;(void)r;(void)p;return TRUE;}\n")
    subprocess.run(["clang","-arch","x86_64","-dynamiclib","-std=gnu11","-O2","-fblocks","-Wall","-Wextra","-Werror","-D__WINESRC__","-DWINE_UNIX_LIB","-I"+str(BUILD/"include"),"-I"+str(SRC/"include"),"-I"+str(SRC/"dlls/winemac.drv"),str(TEST/"mock-host-support.c"),"-o",str(TEST/"libmock-host-support.dylib")],check=True,timeout=60)
    cc=["x86_64-w64-mingw32-gcc","-std=c11","-O2","-Wall","-Wextra","-Werror"]
    subprocess.run(cc+[str(TEST/"main.c"),"-o",str(TEST/"host-contract.exe"),"-luser32","-lgdi32"],check=True,timeout=30)
    subprocess.run(["i686-w64-mingw32-gcc","-std=c11","-O2","-Wall","-Wextra","-Werror",str(TEST/"wow64.c"),"-o",str(TEST/"host-wow64.exe"),"-luser32","-lgdi32"],check=True,timeout=30)
    for name in ("phase-one.dll","phase-two.dll"):
        subprocess.run(cc+["-shared",str(TEST/"marker.c"),"-o",str(TEST/name)],check=True,timeout=30)
    runner.harness.real_path(PACKAGE,directory=True)
    current=runner.harness.process_table()
    runner.require(not any(str(PACKAGE) in row["command"] for pid,row in current.items() if pid!=os.getpid()),"Private candidate has an owner; no image was replaced")
    idle=subprocess.run(["/usr/sbin/lsof","-nP","-Fpc","+D",str(PACKAGE)],capture_output=True,timeout=30)
    runner.require(idle.returncode==1 and not idle.stdout and not idle.stderr,"Private candidate idleness was not established")
    relative={"winemac.so":"x86_64-unix/winemac.so","x86_64-windows/winemac.drv":"x86_64-windows/winemac.drv","i386-windows/winemac.drv":"i386-windows/winemac.drv"}
    build_proof=json.loads((WORK/"paired-driver-build.json").read_text())
    patch=ROOT/"runtime-patches/d3dmetal-wine-11.0-macdrv-host.patch"
    runner.require(build_proof.get("source_built") is True and build_proof.get("apple_payload") is False
                   and build_proof.get("source_patch_sha256")==hashlib.sha256(patch.read_bytes()).hexdigest(),
                   "Paired driver build provenance does not match the current reviewed source patch")
    hashes={}
    for original,target in relative.items():
        src=BUILD/"dlls/winemac.drv"/original;dst=PACKAGE/"runtime/lib/wine"/target
        runner.require(hashlib.sha256(src.read_bytes()).hexdigest()==build_proof["paired_artifacts"].get(original),
                       "A paired driver artifact changed after its source build was recorded")
        shutil.copyfile(src,dst)
        if original.endswith(".so"): builder.PACKAGE.repair_image(PACKAGE,dst)
        hashes[target]=hashlib.sha256(dst.read_bytes()).hexdigest()
    runner.EXPERIMENT=TEST
    session=runner.Session(PACKAGE)
    (TEST/"active-session.json").write_text(json.dumps({"root":str(session.root),"prefix":str(session.prefix)})+"\n")
    result={"passed":False,"apple_payload":False,"paired_driver_sha256":hashes}
    try:
        wine=PACKAGE/"bin/wine";env=session.environment("wined3d")
        code,_=session.invoke([wine,"wineboot.exe","-u"],env,"bootstrap",60)
        runner.require(code==0,"Private Wine bootstrap failed")
        for key,value in (("Auto","1"),("Debugger","false.exe")):
            code,_=session.invoke([wine,"reg.exe","add","HKLM\\Software\\Microsoft\\Windows NT\\CurrentVersion\\AeDebug","/v",key,"/t","REG_SZ","/d",value,"/f"],env,"debugger-"+key,10)
            runner.require(code==0,"Could not suppress fixture crash UI")
        session.stop_and_check_idle()
        for name in ("host-contract.exe","host-wow64.exe","phase-one.dll","phase-two.dll"): shutil.copyfile(TEST/name,session.root/"app"/name)
        state=session.root/"host-state.txt";output=session.root/"host-result.json";gate=session.root/"mapping-gate"
        env.update(CASKRUN_D3DMETAL_SHARED_LIBRARY=str(TEST/"libmock-host-support.dylib"),CASKRUN_HOST_TEST_STATE=str(state),CASKRUN_HOST_TEST_RESULT=str(output),CASKRUN_HOST_TEST_DRIVER=str(PACKAGE/"runtime/lib/wine/x86_64-unix/winemac.so"))
        def inspect(executable="host-contract.exe",pe="x86_64-windows/winemac.drv"):
            table=runner.harness.process_table()
            candidates=[str(pid) for pid,row in table.items() if executable in row["command"] and str(session.root) in row["command"]]
            runner.require(len(candidates)==1,"Owned host fixture process was not unique")
            pid=candidates[0]
            mapped=subprocess.run(["/usr/sbin/lsof","-nP","-a","-p",pid,"-d","txt","-F0pfn"],capture_output=True,timeout=10)
            runner.require(mapped.returncode==0 and not mapped.stderr,"Host fixture mappings could not be inspected")
            (TEST/("mapped-"+executable+".private.lsof")).write_bytes(mapped.stdout)
            for rel in ("x86_64-unix/winemac.so",pe):
                path=PACKAGE/"runtime/lib/wine"/rel
                runner.require(os.fsencode(path) in mapped.stdout and hashlib.sha256(path.read_bytes()).hexdigest()==hashes[rel],"Exact paired host driver was not mapped: "+rel)
            runner.require(b"D3DMetal.framework" not in mapped.stdout and b"libd3dshared.dylib" not in mapped.stdout,"Unexpected Apple payload in host-only fixture")
            result["mapped_paired_driver_verified"]=True
        def dos(path): return "Z:"+str(path).replace("/","\\")
        result["wow64_window_lifecycle_tested"]=arguments.wow64_control
        if result["wow64_window_lifecycle_tested"]:
            gate32=session.root/"mapping-gate-x86"
            env32=session.environment("wined3d")
            code,data=session.invoke([wine,session.root/"app/host-wow64.exe",dos(gate32.with_suffix(".ready")),dos(gate32.with_suffix(".release"))],env32,"host-wow64",30,gate=gate32,inspect=lambda:inspect("host-wow64.exe","i386-windows/winemac.drv"))
            runner.require(code==0 and b'"wow64_window_lifecycle":true' in data,"Paired WoW64 driver failed its owned window lifecycle control")
            result["wow64_window_lifecycle_passed"]=True
            session.stop_and_check_idle()
        fixture=session.root/"app/host-contract.exe"
        args=[wine,fixture,dos(state),dos(session.root/"app/phase-one.dll"),dos(session.root/"app/phase-two.dll"),dos(gate.with_suffix(".ready")),dos(gate.with_suffix(".release")),dos(output)]
        code,data=session.invoke(args,env,"host-contract",45,gate=gate,inspect=inspect)
        runner.require(output.is_file(),"Native host fixture did not finish")
        result.update(json.loads(output.read_text()))
        result.update(exit_code=code,fixture_main_completed=b'"fixture_main_completed":true' in data)
        result["passed"]=result["passed"] and code==0 and result["fixture_main_completed"] and result["mapped_paired_driver_verified"] and (not result["wow64_window_lifecycle_tested"] or result.get("wow64_window_lifecycle_passed") is True)
    except BaseException as error: result.update(passed=False,error=str(error))
    finally:
        diagnostics=session.raw/"host-contract.stderr"
        if diagnostics.is_file() and diagnostics.stat().st_size<=runner.MAX_LOG:
            shutil.copyfile(diagnostics,TEST/"host-contract.stderr")
        try:
            session.stop_and_check_idle();session._validate_owner()
            runner.require(shutil.rmtree.avoids_symlink_attacks,"Safe fixture removal is unavailable")
            shutil.rmtree(session.root);result["cleanup"]="removed_after_no_owners_verified"
        except BaseException as error:result.update(passed=False,cleanup_error=str(error))
        (TEST/"result.json").write_text(json.dumps(result,indent=2)+"\n")
        print(json.dumps(result))
    return 0 if result["passed"] else 1

if __name__=="__main__": sys.exit(main())
