#!/usr/bin/env python3
"""Run the exact Steam bundle selector/routing code without any Wine process.

Real owned files exercise canonical-path and mode guards. Loader exec and the
existing generic Core callback are counted stubs, so no application is launched.
"""
from pathlib import Path
import argparse
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--source', type=Path, default=REPO/'.build/steam-native-bundle-oct14/source')
ARGS, TEST_ARGS = parser.parse_known_args()
SOURCE = ARGS.source.resolve()
if not SOURCE.is_relative_to((REPO/'.build').resolve()):
    parser.error('source must be an isolated tree inside .build')

def function(source, signature):
    start = source.index(signature)
    opening = source.index('{', start)
    level = 1
    end = opening + 1
    while level:
        level += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]

STUB = r'''
#include <assert.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <strings.h>
#include <sys/stat.h>
#include <unistd.h>
#include <limits.h>
#include <errno.h>
typedef int BOOL;
typedef int NTSTATUS;
typedef unsigned short WORD;
#define TRUE 1
#define FALSE 0
#define STATUS_INVALID_IMAGE_FORMAT 9
static char *build_dir,*ntdll_dir,*wineloader;
static char selected[PATH_MAX];
static int callbacks,execs;
static char *build_path(const char *parent,const char *name) {
    char *result=NULL;assert(asprintf(&result,"%s/%s",parent,name)>=0);return result;
}
static char *get_alternate_wineloader(WORD machine) {(void)machine;return NULL;}
static void preloader_exec(char **argv) {
    assert(argv[1]);assert(strlen(argv[1])<sizeof(selected));strcpy(selected,argv[1]);execs++;
}
static char *caskrun_native_app_program(void) {callbacks++;return strdup("/original-game-alias");}
'''
MAIN = r'''
static char data[PATH_MAX],core[PATH_MAX],library[PATH_MAX],ntdll[PATH_MAX],image[PATH_MAX];
static char alias[PATH_MAX],raw[PATH_MAX],game[PATH_MAX],other[PATH_MAX],directory[PATH_MAX];
static unsigned int checks;
static void path(char *target,const char *base,const char *suffix) {
    int length=snprintf(target,PATH_MAX,"%s/%s",base,suffix);assert(length>=0 && length<PATH_MAX);
}
static void reset(void) {
    callbacks=execs=0;selected[0]=0;build_dir=NULL;
    setenv("CASKRUN_STEAM_MODE","1",1);setenv("CASKRUN_NATIVE_CORE",core,1);
    setenv("CASKRUN_NATIVE_DATA",data,1);setenv("CASKRUN_NATIVE_NTDLL",ntdll,1);
    setenv("CASKRUN_NATIVE_BOTTLE","11111111-1111-4111-8111-111111111111",1);
    setenv("CASKRUN_NATIVE_IMAGE",image,1);setenv("CASKRUN_STEAM_NATIVE_APP",alias,1);
    setenv("CASKRUN_NATIVE_EXEC","stale-parent-marker",1);
}
static void route(const char *command,const char *expected,int native,int core_calls) {
    char *argv[]={NULL,NULL,(char*)command,NULL};
    assert(loader_exec(argv,0)==STATUS_INVALID_IMAGE_FORMAT);
    assert(execs==1 && callbacks==core_calls && !strcmp(selected,expected));
    const char *marker=getenv("CASKRUN_NATIVE_EXEC");
    assert(native ? marker && !strcmp(marker,"1") : !marker);
    free(argv[1]);checks++;
}
static void refuse(void) {route("C:\\Steam\\steamwebhelper.exe",raw,0,0);}
int main(int argc,char **argv) {
    assert(argc==2);path(data,argv[1],"data");path(core,argv[1],"fixture-core");
    path(library,argv[1],"runtime/lib/wine/x86_64-unix");ntdll_dir=library;
    path(ntdll,library,"ntdll.so");path(raw,library,"Steam");
    path(image,data,"bottles/11111111-1111-4111-8111-111111111111/prefix/Steam/bin/steamwebhelper.exe");
    path(game,data,"bottles/11111111-1111-4111-8111-111111111111/prefix/Game.exe");
    path(alias,data,"bottles/11111111-1111-4111-8111-111111111111/native-apps/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/Example Steam.app/Contents/MacOS/WindowsApp");
    wineloader="/original-wine-loader";
    reset();route("C:\\Steam\\steamwebhelper.exe",alias,1,0);
    reset();route("C:\\Steam\\STEAMWEBHELPER.EXE",alias,1,0);
    reset();route("C:\\Steam\\steam.exe",raw,0,0);
    reset();setenv("CASKRUN_NATIVE_IMAGE",game,1);route("C:\\Game.exe","/original-game-alias",1,1);
    reset();setenv("CASKRUN_STEAM_MODE","0",1);route("C:\\Steam\\steamwebhelper.exe","/original-game-alias",1,1);
    reset();setenv("CASKRUN_STEAM_MODE","2",1);route("C:\\Steam\\steamwebhelper.exe","/original-game-alias",1,1);
    reset();unsetenv("CASKRUN_STEAM_NATIVE_APP");refuse();
    reset();unsetenv("CASKRUN_NATIVE_CORE");refuse();
    reset();setenv("CASKRUN_NATIVE_BOTTLE","11111111-1111-4111-8111-111111111112",1);refuse();
    reset();path(other,argv[1],"other-ntdll.so");setenv("CASKRUN_NATIVE_NTDLL",other,1);refuse();
    reset();setenv("CASKRUN_NATIVE_IMAGE",game,1);refuse();
    reset();path(other,argv[1],"steamwebhelper.exe");setenv("CASKRUN_NATIVE_IMAGE",other,1);refuse();
    reset();path(other,argv[1],"outside.app/Contents/MacOS/WindowsApp");setenv("CASKRUN_STEAM_NATIVE_APP",other,1);refuse();
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111111/native-apps/a/Example Steam.app/Contents/MacOS/WindowsApp");setenv("CASKRUN_STEAM_NATIVE_APP",other,1);refuse();
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111111/native-apps/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA/Example Steam.app/Contents/MacOS/WindowsApp");setenv("CASKRUN_STEAM_NATIVE_APP",other,1);refuse();
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111111/native-apps/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/.hidden.app/Contents/MacOS/WindowsApp");setenv("CASKRUN_STEAM_NATIVE_APP",other,1);refuse();
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111111/native-apps/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/Example Steam.app/Contents/MacOS/Steam");setenv("CASKRUN_STEAM_NATIVE_APP",other,1);refuse();
    reset();assert(chmod(alias,0777)==0);refuse();assert(chmod(alias,0700)==0);
    reset();strcpy(directory,alias);*strrchr(directory,'/')=0;assert(chmod(directory,0777)==0);refuse();assert(chmod(directory,0700)==0);
    reset();assert(chmod(alias,0600)==0);refuse();assert(chmod(alias,0700)==0);
    reset();path(other,argv[1],"symlink-alias");assert(symlink(alias,other)==0);setenv("CASKRUN_STEAM_NATIVE_APP",other,1);refuse();assert(unlink(other)==0);
    reset();build_dir="/original-source-build";route("C:\\Steam\\steamwebhelper.exe",wineloader,0,0);
    reset();route("C:\\Games\\steam.exe.bak","/original-game-alias",1,1);
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111112/native-apps/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/Example Steam.app/Contents/MacOS/WindowsApp");setenv("CASKRUN_STEAM_NATIVE_APP",other,1);refuse();
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111111/native-apps/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/Example Steam.app/Contents/../Contents/MacOS/WindowsApp");setenv("CASKRUN_STEAM_NATIVE_APP",other,1);refuse();
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111111/native-apps/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/Invalid:.app/Contents/MacOS/WindowsApp");setenv("CASKRUN_STEAM_NATIVE_APP",other,1);refuse();
    reset();char oversized[PATH_MAX+8];memset(oversized,'a',sizeof(oversized)-1);oversized[0]='/';oversized[sizeof(oversized)-1]=0;setenv("CASKRUN_STEAM_NATIVE_APP",oversized,1);refuse();
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111111/native-apps/linked/Example Steam.app/Contents/MacOS/WindowsApp");setenv("CASKRUN_STEAM_NATIVE_APP",other,1);refuse();
    reset();strcpy(directory,alias);char *contents=strstr(directory,"/Contents/MacOS/");assert(contents);*contents=0;path(other,argv[1],"moved-fixture.app");assert(rename(directory,other)==0);assert(symlink(other,directory)==0);refuse();assert(unlink(directory)==0);assert(rename(other,directory)==0);
    reset();path(other,argv[1],"moved-loader");assert(rename(alias,other)==0);assert(symlink(other,alias)==0);refuse();assert(unlink(alias)==0);assert(rename(other,alias)==0);
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111111/prefix/dosdevices/c:/Steam/bin/steamwebhelper.exe");setenv("CASKRUN_NATIVE_IMAGE",other,1);route("C:\\Steam\\steamwebhelper.exe",alias,1,0);
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111111/prefix/dosdevices/x:/steamwebhelper.exe");setenv("CASKRUN_NATIVE_IMAGE",other,1);refuse();
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111111/prefix/dosdevices/c:/Steam/bin/Wrong.exe");setenv("CASKRUN_NATIVE_IMAGE",other,1);refuse();
    reset();path(other,data,"bottles/11111111-1111-4111-8111-111111111111/prefix/final-helper/steamwebhelper.exe");setenv("CASKRUN_NATIVE_IMAGE",other,1);refuse();
    reset();path(library,argv[1],"runtime/components/dxmt/runtime/lib/wine/x86_64-unix");path(other,library,"ntdll.so");setenv("CASKRUN_NATIVE_NTDLL",other,1);ntdll_dir=library;route("C:\\Steam\\steamwebhelper.exe",alias,1,0);
    printf("%u exact loader routing/guard checks passed; no callback for Steam\n",checks);
    return 0;
}
'''

OBJC_STUB = r'''
#import <Foundation/Foundation.h>
#include <assert.h>
#include <stdlib.h>
#include <string.h>
static NSArray *fixtureArguments;
@interface NSProcessInfo (CaskrunFixture)
- (NSArray*)fixtureArguments;
@end
@implementation NSProcessInfo (CaskrunFixture)
- (NSArray*)fixtureArguments { return fixtureArguments; }
@end
'''
OBJC_MAIN = r'''
int main(void) { @autoreleasepool {
    unsigned int checks=0;
    for (unsigned int mode=0;mode<3;mode++) for (unsigned int marker=0;marker<3;marker++) {
        const char *values[]={"0","1","2"};setenv("CASKRUN_STEAM_MODE",values[mode],1);setenv("CASKRUN_NATIVE_EXEC",values[marker],1);
        fixtureArguments=@[@"/bundle/WindowsApp",@"C:\\Steam\\steamwebhelper.exe"];
        assert(CaskrunIsWindowsProcess(@"steamwebhelper.exe")==(mode==1 && marker==1));checks++;
        fixtureArguments=@[@"/bundle/WindowsApp",@"C:\\Games\\ExampleGame.exe"];
        assert(!CaskrunIsWindowsProcess(@"steamwebhelper.exe"));checks++;
    }
    unsetenv("CASKRUN_NATIVE_EXEC");setenv("CASKRUN_STEAM_MODE","1",1);
    fixtureArguments=@[@"/runtime/Steam",@"C:\\Steam\\steamwebhelper.exe"];assert(CaskrunIsWindowsProcess(@"steamwebhelper.exe"));checks++;
    fixtureArguments=@[@"steamwebhelper.exe"];assert(CaskrunIsWindowsProcess(@"steamwebhelper.exe"));checks++;
    fixtureArguments=@[@"/runtime/wine",@"C:\\Steam\\steam.exe"];assert(CaskrunIsWindowsProcess(@"steam.exe"));checks++;
    fixtureArguments=@[@"/unrecognized/Other",@"steamwebhelper.exe"];assert(!CaskrunIsWindowsProcess(@"steamwebhelper.exe"));checks++;
    setenv("CASKRUN_NATIVE_IMAGE","/prefix/Steam/bin/steamwebhelper.exe",1);
    fixtureArguments=@[@"WindowsApp",@"steamwebhelper.exe"];assert(!CaskrunNativeBackgroundProcess());checks++;
    for (NSString *role in @[@"renderer",@"gpu-process",@"utility",@"zygote",@"crashpad-handler"]) {
        fixtureArguments=@[@"WindowsApp",@"steamwebhelper.exe",[NSString stringWithFormat:@"--type=%@",role]];
        assert(CaskrunNativeBackgroundProcess());checks++;
        fixtureArguments=@[@"WindowsApp",@"steamwebhelper.exe",@"--type",role];
        assert(CaskrunNativeBackgroundProcess());checks++;
    }
    fixtureArguments=@[@"WindowsApp",@"steamwebhelper.exe",@"--type=renderer",@"--type=browser"];assert(!CaskrunNativeBackgroundProcess());checks++;
    fixtureArguments=@[@"WindowsApp",@"steamwebhelper.exe",@"--",@"--type=renderer"];assert(!CaskrunNativeBackgroundProcess());checks++;
    printf("%u exact Cocoa loader-name/CEF role scope checks passed\n",checks);
}return 0;}
'''

class BundleTests(unittest.TestCase):
    def test_real_files_and_exact_loader_routing(self):
        s=(SOURCE/'dlls/ntdll/unix/loader.c').read_text()
        names=['static BOOL caskrun_steam_dock_program','static BOOL caskrun_native_identity_enabled',
               'static BOOL caskrun_native_owned_file','static BOOL caskrun_native_bottle_id',
               'static BOOL caskrun_native_steam_directory','static char *caskrun_native_steam_app_program',
               'static NTSTATUS loader_exec']
        with tempfile.TemporaryDirectory(prefix='steam-bundle-',dir=REPO/'.build') as tmp:
            d=Path(tmp);bottle='11111111-1111-4111-8111-111111111111'
            cache=d/'data/bottles'/bottle/'native-apps'
            files=['fixture-core','runtime/lib/wine/x86_64-unix/ntdll.so','other-ntdll.so','steamwebhelper.exe',
                   f'data/bottles/{bottle}/prefix/Steam/bin/steamwebhelper.exe',f'data/bottles/{bottle}/prefix/Game.exe',
                   'outside.app/Contents/MacOS/WindowsApp','runtime/components/dxmt/runtime/lib/wine/x86_64-unix/ntdll.so',
                   f'data/bottles/11111111-1111-4111-8111-111111111112/native-apps/{"a"*64}/Example Steam.app/Contents/MacOS/WindowsApp']
            for key,app,exe in [('a'*64,'Example Steam.app','WindowsApp'),('a','Example Steam.app','WindowsApp'),
                                ('A'*64,'Example Steam.app','WindowsApp'),('a'*64,'.hidden.app','WindowsApp'),
                                ('a'*64,'Example Steam.app','Steam'),('a'*64,'Invalid:.app','WindowsApp')]:
                files.append(str((cache/key/app/'Contents/MacOS'/exe).relative_to(d)))
            for f in files:
                p=d/f;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(b'original fixture');p.chmod(0o700)
            prefix=d/'data/bottles'/bottle/'prefix'
            (prefix/'dosdevices').mkdir()
            (prefix/'dosdevices/c:').symlink_to('../drive_c',target_is_directory=True)
            (prefix/'drive_c/Steam/bin').mkdir(parents=True)
            for name in ['steamwebhelper.exe','Wrong.exe']:
                target=prefix/'drive_c/Steam/bin'/name;target.write_bytes(b'original fixture');target.chmod(0o700)
            (prefix/'dosdevices/x:').symlink_to(d,target_is_directory=True)
            (prefix/'final-helper').mkdir()
            (prefix/'final-helper/steamwebhelper.exe').symlink_to(prefix/'Game.exe')
            (cache/'linked').symlink_to(cache/('a'*64),target_is_directory=True)
            for p in d.rglob('*'):
                if p.is_dir():p.chmod(0o700)
            source=d/'routing.c';source.write_text(STUB+'\n\n'.join(function(s,n) for n in names)+MAIN)
            binary=d/'routing'
            subprocess.run(['clang','-Wall','-Wextra','-Werror','-fsanitize=address,undefined',str(source),'-o',str(binary)],check=True)
            print(subprocess.check_output([str(binary),str(d)],text=True).strip())

    def test_exact_cocoa_process_matcher(self):
        s=(SOURCE/'dlls/winemac.drv/cocoa_app.m').read_text()
        names=['static BOOL CaskrunSteamDockEnabled','static NSString* CaskrunArgumentBasename','static BOOL CaskrunIsWindowsProcess','static NSString* CaskrunNativeImageName','static BOOL CaskrunNativeBackgroundProcess']
        extracted='\n\n'.join(function(s,n) for n in names).replace('[[NSProcessInfo processInfo] arguments]','[[NSProcessInfo processInfo] fixtureArguments]')
        with tempfile.TemporaryDirectory(prefix='steam-matcher-',dir=REPO/'.build') as tmp:
            d=Path(tmp);source=d/'matcher.m';source.write_text(OBJC_STUB+extracted+OBJC_MAIN);binary=d/'matcher'
            subprocess.run(['clang','-Wall','-Wextra','-Werror','-framework','Foundation',str(source),'-o',str(binary)],check=True)
            print(subprocess.check_output([str(binary)],text=True).strip())

if __name__=='__main__':unittest.main(argv=[__file__,*TEST_ARGS])
