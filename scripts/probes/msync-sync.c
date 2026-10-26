/* SPDX-License-Identifier: MIT
 * Original bounded Win32 synchronisation probe for the Wine11 MSync port.
 * No publisher binaries, account data, clipboard, network or persistent files.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <winternl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static unsigned checks, failures;
#define CHECK(expr, label) do { ++checks; if (!(expr)) { ++failures; printf("FAIL %s (error=%lu)\n", label, GetLastError()); } else printf("PASS %s\n",label); fflush(stdout); } while(0)
static DWORD WINAPI watchdog(void *unused) { (void)unused; Sleep(25000); fprintf(stderr,"FAIL watchdog\n"); ExitProcess(90); return 0; }
static DWORD WINAPI signal_later(void *p) { Sleep(40); SetEvent((HANDLE)p); return 7; }
static DWORD WINAPI own_then_exit(void *p) { WaitForSingleObject((HANDLE)p,2000); return 0; }
static VOID CALLBACK apc(ULONG_PTR p) { InterlockedIncrement((LONG *)p); }
struct apc_info { HANDLE thread, ready; LONG called; DWORD result; };
static DWORD WINAPI alert_wait(void *p) { struct apc_info *i=p; SetEvent(i->ready); i->result=SleepEx(2000,TRUE); return 0; }
static DWORD WINAPI queue_later(void *p) { struct apc_info *i=p; WaitForSingleObject(i->ready,2000); QueueUserAPC(apc,i->thread,(ULONG_PTR)&i->called); return 0; }
struct wait_stress { HANDLE mutex, sem; LONG failures; };
static DWORD WINAPI stress_recursive(void *p) {
 struct wait_stress *i=p; HANDLE objects[2]={i->mutex,i->sem}; unsigned n;
 for(n=0;n<300;n++) {
  if(WaitForSingleObject(i->mutex,2000)!=WAIT_OBJECT_0) {InterlockedIncrement(&i->failures);break;}
  if(WaitForMultipleObjects(2,objects,TRUE,2000)!=WAIT_OBJECT_0) {InterlockedIncrement(&i->failures);ReleaseMutex(i->mutex);break;}
  if(!ReleaseMutex(i->mutex)||!ReleaseSemaphore(i->sem,1,NULL)||!ReleaseMutex(i->mutex))InterlockedIncrement(&i->failures);
 }
 return 0;
}
static DWORD WINAPI stress_semaphore(void *p) {
 struct wait_stress *i=p; unsigned n;
 for(n=0;n<2000;n++)if(WaitForSingleObject(i->sem,2000)==WAIT_OBJECT_0)ReleaseSemaphore(i->sem,1,NULL);else {InterlockedIncrement(&i->failures);break;}
 return 0;
}
struct pool_stress { HANDLE ready, events[2]; LONG failures; };
static DWORD WINAPI stress_pool(void *p) {
 struct pool_stress *i=p; ReleaseSemaphore(i->ready,1,NULL);
 if(WaitForMultipleObjects(2,i->events,FALSE,5000)!=WAIT_OBJECT_0)InterlockedIncrement(&i->failures);
 return 0;
}
static int object_type(HANDLE h,const wchar_t *expected) {
 typedef LONG (NTAPI *query_fn)(HANDLE,ULONG,PVOID,ULONG,PULONG);
 query_fn query=(query_fn)(void *)GetProcAddress(GetModuleHandleA("ntdll.dll"),"NtQueryObject");
 union { void *align; char bytes[4096]; } info; UNICODE_STRING *name=(UNICODE_STRING *)info.bytes;
 if(!query||query(h,2,info.bytes,sizeof(info.bytes),NULL))return 0;
 return name->Length==wcslen(expected)*sizeof(wchar_t)&&!memcmp(name->Buffer,expected,name->Length);
}
static int child_mode(const char *mode, const char *name) {
 HANDLE h;
 if (!strcmp(mode,"--hold") && !strcmp(name,"5000")) { Sleep(5000); return 0; }
 if (!strcmp(mode,"--child-event")) { h=OpenEventA(EVENT_MODIFY_STATE,FALSE,name); if(!h)return 10; SetEvent(h); CloseHandle(h); return 0; }
 if (!strcmp(mode,"--child-mutex")) { h=OpenMutexA(SYNCHRONIZE,FALSE,name); if(!h)return 11; return WaitForSingleObject(h,2000)==WAIT_OBJECT_0?0:12; }
 return 13;
}
static BOOL spawn(const char *mode,const char *name, PROCESS_INFORMATION *pi) {
 char exe[MAX_PATH],cmd[2*MAX_PATH]; STARTUPINFOA si; ZeroMemory(&si,sizeof(si)); si.cb=sizeof(si); ZeroMemory(pi,sizeof(*pi));
 GetModuleFileNameA(NULL,exe,sizeof(exe)); snprintf(cmd,sizeof(cmd),"\"%s\" %s %s",exe,mode,name); return CreateProcessA(NULL,cmd,NULL,NULL,FALSE,0,NULL,NULL,&si,pi);
}
int main(int argc,char **argv) {
 HANDLE a,b,m,s,t,ro,dup,v[2]; LONG old; DWORD r,code; ULONGLONG tick; unsigned n; char name[96]; PROCESS_INFORMATION pi; struct apc_info ai; struct wait_stress stress; HANDLE workers[3]; struct pool_stress pool; HANDLE pool_threads[40]; unsigned round;
 if(argc==3)return child_mode(argv[1],argv[2]);
 CreateThread(NULL,0,watchdog,NULL,0,NULL);
 printf("ARCH %u\n",(unsigned)(sizeof(void*)*8));
 a=CreateEventA(NULL,TRUE,FALSE,NULL); b=CreateEventA(NULL,FALSE,FALSE,NULL); s=CreateSemaphoreA(NULL,0,3,NULL); m=CreateMutexA(NULL,FALSE,NULL);
 CHECK(a&&b&&s&&m,"create primitives");
 CHECK(object_type(a,L"Event")&&object_type(s,L"Semaphore")&&object_type(m,L"Mutant"),"native object type metadata");
 CHECK(WaitForSingleObject(a,0)==WAIT_TIMEOUT,"manual initial unsignalled");
 CHECK(DuplicateHandle(GetCurrentProcess(),a,GetCurrentProcess(),&ro,SYNCHRONIZE,FALSE,0),"duplicate readonly event");SetLastError(0);CHECK(!SetEvent(ro)&&GetLastError()==ERROR_ACCESS_DENIED,"readonly event denies signal");CloseHandle(ro);
 CHECK(DuplicateHandle(GetCurrentProcess(),a,GetCurrentProcess(),&ro,EVENT_MODIFY_STATE,FALSE,0),"duplicate signal-only event");CHECK(SetEvent(ro),"signal-only event permits signal");SetLastError(0);CHECK(WaitForSingleObject(ro,0)==WAIT_FAILED&&GetLastError()==ERROR_ACCESS_DENIED,"signal-only event denies wait");ResetEvent(a);CloseHandle(ro);
 SetLastError(0);CHECK(!ResetEvent(s)&&GetLastError()==ERROR_INVALID_HANDLE,"reset rejects semaphore type");SetLastError(0);CHECK(!ReleaseSemaphore(a,1,NULL)&&GetLastError()==ERROR_INVALID_HANDLE,"release rejects event type");
 SetEvent(a); CHECK(WaitForSingleObject(a,0)==WAIT_OBJECT_0 && WaitForSingleObject(a,0)==WAIT_OBJECT_0,"manual event persistent"); ResetEvent(a);
 SetEvent(b); CHECK(WaitForSingleObject(b,0)==WAIT_OBJECT_0 && WaitForSingleObject(b,0)==WAIT_TIMEOUT,"auto event consumed once");
 old=99; CHECK(ReleaseSemaphore(s,2,&old)&&old==0,"semaphore release previous count");
 CHECK(WaitForSingleObject(s,0)==WAIT_OBJECT_0&&WaitForSingleObject(s,0)==WAIT_OBJECT_0&&WaitForSingleObject(s,0)==WAIT_TIMEOUT,"semaphore count consumed");
 SetLastError(0); CHECK(!ReleaseSemaphore(s,4,NULL)&&GetLastError()==ERROR_TOO_MANY_POSTS,"semaphore maximum enforced");
 CHECK(WaitForSingleObject(m,0)==WAIT_OBJECT_0&&WaitForSingleObject(m,0)==WAIT_OBJECT_0,"recursive mutex"); CHECK(ReleaseMutex(m)&&ReleaseMutex(m),"recursive mutex release"); CHECK(!ReleaseMutex(m)&&GetLastError()==ERROR_NOT_OWNER,"unowned mutex rejected");
 v[0]=a;v[1]=b;SetEvent(b); CHECK(WaitForMultipleObjects(2,v,FALSE,0)==WAIT_OBJECT_0+1,"wait any index");
 SetEvent(a);SetEvent(b);CHECK(WaitForMultipleObjects(2,v,TRUE,0)==WAIT_OBJECT_0,"wait all events");CHECK(WaitForSingleObject(b,0)==WAIT_TIMEOUT,"wait all consumes auto event");ResetEvent(a);
 tick=GetTickCount64();r=WaitForSingleObject(a,60);tick=GetTickCount64()-tick;CHECK(r==WAIT_TIMEOUT&&tick>=35&&tick<2000,"bounded relative timeout");
 t=CreateThread(NULL,0,signal_later,a,0,NULL);CHECK(WaitForSingleObject(a,2000)==WAIT_OBJECT_0,"thread cross wake");CHECK(WaitForSingleObject(t,2000)==WAIT_OBJECT_0&&GetExitCodeThread(t,&code)&&code==7,"thread exit signal");CloseHandle(t);ResetEvent(a);
 t=CreateThread(NULL,0,own_then_exit,m,0,NULL);WaitForSingleObject(t,2000);CHECK(WaitForSingleObject(m,2000)==WAIT_ABANDONED_0,"thread abandoned mutex");ReleaseMutex(m);CloseHandle(t);
 t=CreateThread(NULL,0,own_then_exit,m,0,NULL);WaitForSingleObject(t,2000);SetEvent(a);v[0]=m;v[1]=a;CHECK(WaitForMultipleObjects(2,v,TRUE,2000)==WAIT_ABANDONED_0,"wait all accepts abandoned mutex");ReleaseMutex(m);ResetEvent(a);CloseHandle(t);

 ZeroMemory(&ai,sizeof(ai));ai.ready=CreateEventA(NULL,TRUE,FALSE,NULL);ai.thread=CreateThread(NULL,0,alert_wait,&ai,CREATE_SUSPENDED,NULL);t=CreateThread(NULL,0,queue_later,&ai,0,NULL);ResumeThread(ai.thread);WaitForSingleObject(ai.thread,3000);WaitForSingleObject(t,3000);CHECK(ai.result==WAIT_IO_COMPLETION&&ai.called==1,"APC cancels alertable wait exactly once");CloseHandle(t);CloseHandle(ai.thread);CloseHandle(ai.ready);
 snprintf(name,sizeof(name),"Local\\CaskrunSyncEvent_%lu",GetCurrentProcessId());t=CreateEventA(NULL,TRUE,FALSE,name);CHECK(spawn("--child-event",name,&pi),"spawn cross process event");CHECK(WaitForSingleObject(t,2000)==WAIT_OBJECT_0,"cross process event");CHECK(WaitForSingleObject(pi.hProcess,2000)==WAIT_OBJECT_0&&GetExitCodeProcess(pi.hProcess,&code)&&!code,"process exit signal");CloseHandle(pi.hProcess);CloseHandle(pi.hThread);CloseHandle(t);
 snprintf(name,sizeof(name),"Local\\CaskrunSyncMutex_%lu",GetCurrentProcessId());t=CreateMutexA(NULL,FALSE,name);CHECK(spawn("--child-mutex",name,&pi),"spawn mutex owner");CHECK(WaitForSingleObject(pi.hProcess,2000)==WAIT_OBJECT_0,"mutex owner process exit");CHECK(WaitForSingleObject(t,2000)==WAIT_ABANDONED_0,"process abandoned mutex");ReleaseMutex(t);CloseHandle(pi.hProcess);CloseHandle(pi.hThread);CloseHandle(t);
 ZeroMemory(&stress,sizeof(stress));stress.mutex=CreateMutexA(NULL,FALSE,NULL);stress.sem=CreateSemaphoreA(NULL,1,1,NULL);workers[0]=CreateThread(NULL,0,stress_recursive,&stress,0,NULL);workers[1]=CreateThread(NULL,0,stress_recursive,&stress,0,NULL);workers[2]=CreateThread(NULL,0,stress_semaphore,&stress,0,NULL);r=WaitForMultipleObjects(3,workers,TRUE,7000);CHECK(r==WAIT_OBJECT_0&&stress.failures==0,"contended wait all preserves recursive mutex ownership");for(n=0;n<3;n++)CloseHandle(workers[n]);CloseHandle(stress.sem);CloseHandle(stress.mutex);
 ZeroMemory(&pool,sizeof(pool));pool.ready=CreateSemaphoreA(NULL,0,40,NULL);pool.events[0]=CreateEventA(NULL,TRUE,FALSE,NULL);pool.events[1]=CreateEventA(NULL,FALSE,FALSE,NULL);
 for(round=0;round<3;round++){ResetEvent(pool.events[0]);for(n=0;n<40;n++)pool_threads[n]=CreateThread(NULL,0,stress_pool,&pool,0,NULL);for(n=0;n<40;n++)if(WaitForSingleObject(pool.ready,2000)!=WAIT_OBJECT_0)InterlockedIncrement(&pool.failures);Sleep(30);SetEvent(pool.events[0]);if(WaitForMultipleObjects(40,pool_threads,TRUE,5000)!=WAIT_OBJECT_0)InterlockedIncrement(&pool.failures);for(n=0;n<40;n++)CloseHandle(pool_threads[n]);}
 CHECK(pool.failures==0,"forty concurrent multiwaits recycle Mach semaphore pool");CloseHandle(pool.ready);CloseHandle(pool.events[0]);CloseHandle(pool.events[1]);
 for(n=0;n<2000;n++){t=CreateEventA(NULL,FALSE,TRUE,NULL);r=WaitForSingleObject(t,0);CloseHandle(t);if(r!=WAIT_OBJECT_0)break;t=CreateEventA(NULL,FALSE,FALSE,NULL);r=WaitForSingleObject(t,0);CloseHandle(t);if(r!=WAIT_TIMEOUT)break;}CHECK(n==2000,"handle close and reuse does not retain state");
 for(n=0;n<1000;n++){t=CreateEventA(NULL,FALSE,TRUE,NULL);WaitForSingleObject(t,0);if(!DuplicateHandle(GetCurrentProcess(),t,GetCurrentProcess(),&dup,0,FALSE,DUPLICATE_SAME_ACCESS|DUPLICATE_CLOSE_SOURCE))break;CloseHandle(dup);t=CreateEventA(NULL,FALSE,FALSE,NULL);r=WaitForSingleObject(t,0);CloseHandle(t);if(r!=WAIT_TIMEOUT)break;}CHECK(n==1000,"duplicate close source invalidates reused handle cache");
 CloseHandle(a);CloseHandle(b);CloseHandle(s);CloseHandle(m);
 printf("RESULT checks=%u failures=%u\n",checks,failures);return failures?1:0;
}
