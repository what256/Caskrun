#!/usr/bin/env python3
"""Exercise the original native-editing patch with synthetic data, never a clipboard.

The five production Wine objects are compiled separately by the runtime build.
This harness compiles the exact patched keyboard handler against controlled OS
responses, so refusal paths and modifier restoration run without user processes.
"""
from pathlib import Path
import argparse
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
PARSER = argparse.ArgumentParser()
PARSER.add_argument("--source", type=Path, default=REPO / ".build/native-text-editing-oct14/wine-candidate/src/wine-11.0")
ARGS, TEST_ARGS = PARSER.parse_known_args()
SOURCE = ARGS.source.resolve()
if not SOURCE.is_relative_to((REPO / ".build").resolve()):
    PARSER.error("test source must be an isolated tree inside .build")


C_STUB = r'''
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef unsigned char BYTE;
typedef unsigned short WORD;
typedef int BOOL;
typedef void *HWND;
#define TRUE 1
#define FALSE 0
#define GA_ROOT 2
#define QS_MOUSEBUTTON 4
#define GUI_INMENUMODE 4
#define GUI_INMOVESIZE 2
#define ARRAY_SIZE(a) (sizeof(a)/sizeof((a)[0]))
#define VK_PACKET 0xe7
#define VK_MENU 0x12
#define VK_CONTROL 0x11
#define VK_SHIFT 0x10
#define VK_LSHIFT 0xa0
#define VK_RSHIFT 0xa1
#define VK_LCONTROL 0xa2
#define VK_RCONTROL 0xa3
#define VK_LMENU 0xa4
#define VK_RMENU 0xa5
#define VK_LWIN 0x5b
#define VK_RWIN 0x5c
#define KEYEVENTF_EXTENDEDKEY 1
#define KEYEVENTF_KEYUP 2
typedef struct { unsigned int cbSize,flags; HWND hwndFocus,hwndMenuOwner,hwndCapture; } GUITHREADINFO;
typedef struct { struct { unsigned int vkey; unsigned long time_ms; void *foreground; BOOL context_menu; } native_text_edit; } macdrv_event;
struct macdrv_thread_data { WORD keyc2vkey[128], keyc2scan[128]; void *queue; };
static struct macdrv_thread_data thread_data;
static const WORD modifier_keys[] = {VK_LSHIFT,VK_RSHIFT,VK_LCONTROL,VK_RCONTROL,VK_LMENU,VK_RMENU,VK_LWIN,VK_RWIN};
static BYTE initial[256], live[256];
static HWND foreground, focused, mapped_root;
static unsigned int gui_flags;
static int pending_cocoa_buttons,pending_windows_buttons;
static int macdrv_queue_has_mouse_button_events(void *queue) {(void)queue;return pending_cocoa_buttons;}
static unsigned int NtUserGetQueueStatus(unsigned int flags) {(void)flags;return pending_windows_buttons?QS_MOUSEBUTTON<<16:0;}
static int state_ok, info_ok, focus_changes, foreground_calls, sent, letters_down, letters_up;
static unsigned int expected_letter;
static struct macdrv_thread_data *macdrv_thread_data(void) { return &thread_data; }
static HWND NtUserGetAncestor(HWND hwnd, int flags) { (void)flags; return hwnd == (HWND)123 || hwnd == (HWND)456 ? mapped_root : hwnd; }
static HWND NtUserGetForegroundWindow(void) { return focus_changes && foreground_calls++ ? (HWND)999 : foreground; }
static unsigned int NtUserGetWindowThread(HWND hwnd, void *process) { (void)hwnd;(void)process;return 1; }
static BOOL NtUserGetGUIThreadInfo(unsigned int thread, GUITHREADINFO *info) { (void)thread;info->hwndFocus=focused;info->flags=gui_flags;return info_ok; }
static BOOL NtUserGetAsyncKeyboardState(BYTE *state) { memcpy(state,live,256);return state_ok; }
static WORD normalize_key(WORD vkey, unsigned int flags);
static void macdrv_send_keyboard_input(HWND hwnd, WORD key, WORD scan, unsigned int flags, unsigned int time)
{
    (void)scan;
    key=normalize_key(key,flags);
    assert(hwnd == (HWND)321); /* every event is explicitly addressed; never NULL */
    assert(time == 42);
    if (key == expected_letter) {
        if (flags & KEYEVENTF_KEYUP) letters_up++;
        else {
            letters_down++;
            assert((live[VK_LCONTROL]|live[VK_RCONTROL]) & 0x80);
            assert(!(live[VK_LSHIFT]|live[VK_RSHIFT]|live[VK_LMENU]|live[VK_RMENU]|live[VK_LWIN]|live[VK_RWIN]));
        }
    }
    live[key] = flags & KEYEVENTF_KEYUP ? 0 : 0x80;
    sent++;
}
'''
C_MAIN = r'''
static void reset(unsigned int mask, unsigned int letter)
{
    unsigned int i;
    memset(initial,0,sizeof(initial));memset(&thread_data,0,sizeof(thread_data));
    for(i=0;i<8;i++) if(mask & (1u<<i)) initial[modifier_keys[i]]=0x80;
    memcpy(live,initial,256);
    mapped_root=foreground=(HWND)321;focused=(HWND)456;
    pending_cocoa_buttons=pending_windows_buttons=gui_flags=0;info_ok=state_ok=1;focus_changes=foreground_calls=sent=letters_down=letters_up=0;
    expected_letter=letter;thread_data.keyc2vkey[0]=letter;thread_data.keyc2scan[0]=0x2f;
}
int main(void)
{
    unsigned int mask,i,context,cases=0,refusals=0;
    const unsigned int actions[]={'A','C','V','X'};
    macdrv_event event={0};event.native_text_edit.time_ms=42;event.native_text_edit.foreground=(void*)321;
    for(context=0;context<2;context++) for(i=0;i<4;i++) for(mask=0;mask<256;mask++) {
        reset(mask,actions[i]);event.native_text_edit.vkey=actions[i];event.native_text_edit.context_menu=context;
        macdrv_native_text_edit((HWND)123,&event);
        assert(letters_down==1 && letters_up==1 && sent>=2);
        assert(!memcmp(live,initial,256));cases++;
    }
    for(i=0;i<15;i++) {
        reset(0,'V');event.native_text_edit.vkey='V';event.native_text_edit.foreground=(void*)321;event.native_text_edit.context_menu=0;
        switch(i) {
        case 0:event.native_text_edit.vkey='Q';break;
        case 1:event.native_text_edit.foreground=NULL;break;
        case 2:mapped_root=(HWND)999;break;
        case 3:foreground=(HWND)999;break;
        case 4:focused=(HWND)999;break;
        case 5:focused=0;break;
        case 6:info_ok=0;break;
        case 7:state_ok=0;break;
        case 8:live['V']=0x80;break;
        case 9:thread_data.keyc2scan[0]=0;break;
        case 10:focus_changes=1;break;
        case 11:gui_flags=GUI_INMENUMODE;break;
        case 12:gui_flags=GUI_INMOVESIZE;break;
        case 13:event.native_text_edit.context_menu=1;pending_cocoa_buttons=1;break;
        case 14:event.native_text_edit.context_menu=1;pending_windows_buttons=1;break;
        }
        macdrv_native_text_edit((HWND)123,&event);assert(sent==0);refusals++;
    }
    reset(0,'V');event.native_text_edit.vkey='V';event.native_text_edit.foreground=(void*)321;event.native_text_edit.context_menu=0;
    macdrv_native_text_edit(0,&event);assert(sent==0);refusals++;
    printf("%u modifier/action cases, %u foreground/focus/key refusals passed\n",cases,refusals);
    return 0;
}
'''


QUERY_STUB = r"""
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef int BOOL;
typedef uint32_t DWORD;
typedef void *HWND;
typedef uint16_t WCHAR;
typedef struct {uint16_t Length,MaximumLength;WCHAR *Buffer;} UNICODE_STRING;
#define TRUE 1
#define FALSE 0
#define GA_ROOT 2
#define GA_ROOTOWNER 3
#define OBJID_CLIENT -4
#define GWL_STYLE -16
#define GWL_EXSTYLE -20
#define GW_OWNER 4
#define WS_POPUP 0x80000000u
#define WS_CHILD 0x40000000u
#define WS_DISABLED 0x08000000u
#define WS_EX_TRANSPARENT 0x20u
#define WS_EX_TOOLWINDOW 0x80u
#define WS_EX_LAYERED 0x80000u
#define WS_EX_NOACTIVATE 0x08000000u
#define WM_CANCELMODE 0x1fu
#define ARRAY_SIZE(a) (sizeof(a)/sizeof((a)[0]))
#define QS_MOUSEBUTTON 4
#define GUI_INMENUMODE 4
#define GUI_INMOVESIZE 2
enum {NATIVE_TEXT_CONTEXT_PENDING,NATIVE_TEXT_CONTEXT_DISMISS_REQUESTED,NATIVE_TEXT_CONTEXT_READY,NATIVE_TEXT_CONTEXT_REFUSED};
typedef struct {unsigned int cbSize,flags;HWND hwndFocus,hwndMenuOwner,hwndCapture,hwndActive;} GUITHREADINFO;
typedef struct {unsigned int cbSize;BOOL fBarFocused;} MENUBARINFO;
typedef struct {void *window;struct {void *foreground,*focus,*popup;unsigned int serial,phase;BOOL menu_seen;}native_text_context;}macdrv_query;
struct macdrv_thread_data {void *queue;};
static struct macdrv_thread_data data;
struct macdrv_win_data {void *cocoa_window;};
static struct macdrv_win_data win;
static int win_ok;
static struct macdrv_win_data *get_win_data(HWND hwnd){(void)hwnd;return win_ok?&win:0;}
static void release_win_data(struct macdrv_win_data *w){(void)w;}
static HWND root,foreground,focus,owner,capture,owner_root,active,popup_owner;
static unsigned int flags,root_thread,owner_thread,popup_thread;
static int serial_current,pending_cocoa,pending_windows,info_ok,end_ok,end_calls,foreground_changes,foreground_calls,menu_ok,bar_focused;
static int popup_exists,popup_visible,popup_class_mode,post_ok,cancel_calls,gui_calls,gui_change;
static DWORD popup_style,popup_exstyle;
static struct macdrv_thread_data *macdrv_thread_data(void){return &data;}
static int macdrv_native_text_context_current(unsigned int serial){return serial==17&&serial_current;}
static HWND NtUserGetAncestor(HWND hwnd,int what){if(hwnd==(HWND)777)return what==GA_ROOTOWNER?owner_root:hwnd;return hwnd==(HWND)123||hwnd==(HWND)456||hwnd==(HWND)457?root:hwnd;}
static HWND NtUserGetForegroundWindow(void){return foreground_changes&&foreground_calls++?(HWND)999:foreground;}
static unsigned int NtUserGetWindowThread(HWND hwnd,void *process){(void)process;return hwnd==(HWND)777?owner_thread:hwnd==(HWND)888?popup_thread:root_thread;}
static unsigned int GetCurrentThreadId(void){return 11;}
static BOOL NtUserGetGUIThreadInfo(unsigned int thread,GUITHREADINFO *info){
 (void)thread;info->flags=flags;info->hwndFocus=focus;info->hwndMenuOwner=owner;info->hwndCapture=capture;info->hwndActive=active;
 if(gui_calls++&&gui_change){switch(gui_change){case 1:info->hwndCapture=(HWND)889;break;case 2:info->hwndFocus=(HWND)457;break;case 3:info->hwndActive=(HWND)999;break;case 4:info->flags|=GUI_INMENUMODE;break;case 5:info->flags|=GUI_INMOVESIZE;break;case 6:info->hwndMenuOwner=(HWND)777;break;case 7:return FALSE;case 8:popup_owner=(HWND)999;break;case 9:popup_visible=0;break;case 10:popup_class_mode=1;break;case 11:popup_exists=0;break;case 12:popup_exstyle|=WS_EX_TRANSPARENT;break;}}
 return info_ok;}
static int macdrv_queue_has_mouse_button_events(void *queue){(void)queue;return pending_cocoa;}
static unsigned int NtUserGetQueueStatus(unsigned int mask){(void)mask;return pending_windows?QS_MOUSEBUTTON<<16:0;}
static BOOL NtUserGetMenuBarInfo(HWND hwnd,int id,int item,MENUBARINFO*menu){assert(hwnd==(HWND)777&&id==OBJID_CLIENT&&item==0);menu->fBarFocused=bar_focused;return menu_ok;}
static BOOL NtUserEndMenu(void){end_calls++;return end_ok;}
static BOOL NtUserIsWindow(HWND hwnd){return hwnd==(HWND)888&&popup_exists;}
static BOOL NtUserIsWindowVisible(HWND hwnd){return NtUserIsWindow(hwnd)&&popup_visible;}
static HWND NtUserGetWindowRelative(HWND hwnd,int relation){assert(hwnd==(HWND)888&&relation==GW_OWNER);return popup_owner;}
static DWORD NtUserGetWindowLongW(HWND hwnd,int index){assert(hwnd==(HWND)888);return index==GWL_STYLE?popup_style:popup_exstyle;}
static int NtUserGetClassName(HWND hwnd,int real,UNICODE_STRING *name){
 static const WCHAR expected[]={'C','h','r','o','m','e','_','W','i','d','g','e','t','W','i','n','_','1',0};
 assert(hwnd==(HWND)888&&!real&&name->MaximumLength==sizeof(expected)+sizeof(WCHAR));memcpy(name->Buffer,expected,sizeof(expected));
 if(popup_class_mode==1)name->Buffer[17]='0';
 if(popup_class_mode==2){name->Buffer[18]='S';name->Buffer[19]=0;return ARRAY_SIZE(expected);}
 if(popup_class_mode==3)return 0;
 return ARRAY_SIZE(expected)-1;}
static BOOL NtUserPostMessage(HWND hwnd,unsigned int message,uintptr_t wp,intptr_t lp){
 assert(hwnd==(HWND)888&&message==WM_CANCELMODE&&!wp&&!lp);cancel_calls++;return post_ok;}
"""
QUERY_MAIN = r"""
static macdrv_query reset(void){
 macdrv_query q={.window=(void*)444,.native_text_context={.foreground=(HWND)321,.serial=17}};
 win_ok=1;win.cocoa_window=(void*)444;
 root=foreground=owner_root=active=popup_owner=(HWND)321;focus=(HWND)456;owner=(HWND)777;capture=0;menu_ok=bar_focused=1;
 flags=GUI_INMENUMODE;root_thread=owner_thread=popup_thread=11;serial_current=info_ok=end_ok=1;
 pending_cocoa=pending_windows=end_calls=foreground_changes=foreground_calls=0;
 popup_exists=popup_visible=post_ok=1;popup_class_mode=cancel_calls=gui_calls=gui_change=0;
 popup_style=WS_POPUP;popup_exstyle=WS_EX_TOOLWINDOW|WS_EX_NOACTIVATE|WS_EX_LAYERED;
 return q;
}
static macdrv_query captured(void){macdrv_query q=reset();flags=0;owner=0;capture=(HWND)888;return q;}
static void requested(macdrv_query*q){
 assert(macdrv_native_text_context((HWND)123,q));assert(q->native_text_context.phase==NATIVE_TEXT_CONTEXT_DISMISS_REQUESTED);
 assert(q->native_text_context.popup==(HWND)888&&q->native_text_context.focus==(HWND)456&&cancel_calls==1&&end_calls==0);
 q->native_text_context.menu_seen=1;
}
int main(void){
 macdrv_query q=reset();assert(macdrv_native_text_context((HWND)123,&q));
 assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_DISMISS_REQUESTED&&end_calls==1);
 /* EndMenu posts cancellation: tracking must actually finish before READY. */
 q.native_text_context.menu_seen=1;flags=0;owner=0;
 assert(macdrv_native_text_context((HWND)123,&q));assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_READY&&end_calls==1);
 for(int i=0;i<19;i++){
  q=reset();switch(i){
   case 0:q.native_text_context.serial=16;break;
   case 1:serial_current=0;break;
   case 2:root=(HWND)999;break;
   case 3:foreground=(HWND)999;break;
   case 4:root_thread=12;break;
   case 5:info_ok=0;break;
   case 6:focus=0;break;
   case 7:focus=(HWND)999;break;
   case 8:flags|=GUI_INMOVESIZE;break;
   case 9:owner=0;break;
   case 10:owner=(HWND)999;break;
   case 11:owner_thread=12;break;
   case 12:foreground_changes=1;break;
   case 13:q.native_text_context.foreground=0;break;
   case 14:win_ok=0;break;
   case 15:win.cocoa_window=(void*)555;break;
   case 16:owner_root=(HWND)999;break;
   case 17:menu_ok=0;break;
   case 18:bar_focused=0;break;
  }
  macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_REFUSED&&end_calls==0);
 }
 q=reset();q.native_text_context.menu_seen=1;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_PENDING&&end_calls==0);
 q=reset();pending_cocoa=1;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_PENDING&&end_calls==0);
 q=reset();pending_windows=1;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_PENDING&&end_calls==0);
 q=reset();flags=0;owner=0;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_PENDING&&end_calls==0);
 q=reset();end_ok=0;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_REFUSED&&end_calls==1);
 q=reset();flags=0;owner=0;capture=(HWND)456;q.native_text_context.menu_seen=1;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_REFUSED&&end_calls==0);
 q=reset();flags=0;owner=(HWND)777;q.native_text_context.menu_seen=1;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_REFUSED&&end_calls==0);
 q=reset();macdrv_native_text_context(0,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_REFUSED&&end_calls==0);
 puts("same-thread menu cancellation, confirmed exit, pending input and 27 refusal/fallback cases passed");

 /* Exact captured popup: posting is a request, never proof of disappearance. */
 q=captured();requested(&q);
 for(int i=0;i<64;i++){macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_PENDING&&cancel_calls==1);}
 popup_visible=0;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_PENDING&&cancel_calls==1);
 capture=0;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_READY&&cancel_calls==1);
 q=captured();requested(&q);capture=0;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_PENDING);popup_visible=0;
 macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_READY&&cancel_calls==1);
 q=captured();requested(&q);popup_exists=0;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_PENDING);capture=0;
 macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_READY&&cancel_calls==1);
 /* No pending buttons may be overtaken, and focus is recorded after drain. */
 for(int i=0;i<2;i++){
  q=captured();pending_cocoa=i==0;pending_windows=i==1;macdrv_native_text_context((HWND)123,&q);
  assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_PENDING&&!q.native_text_context.focus&&!q.native_text_context.popup&&!cancel_calls);
 }
 /* Initial refusal guards: no callback and no cancellation of an unknown UI. */
 for(int i=0;i<33;i++){
  q=captured();switch(i){
   case 0:popup_exists=0;break;
   case 1:popup_visible=0;break;
   case 2:popup_class_mode=1;break;
   case 3:popup_class_mode=2;break;
   case 4:popup_class_mode=3;break;
   case 5:popup_owner=(HWND)999;break;
   case 6:popup_owner=0;break;
   case 7:popup_thread=12;break;
   case 8:popup_style=0;break;
   case 9:popup_style|=WS_CHILD;break;
   case 10:popup_style|=WS_DISABLED;break;
   case 11:popup_exstyle&=~WS_EX_TOOLWINDOW;break;
   case 12:popup_exstyle&=~WS_EX_NOACTIVATE;break;
   case 13:popup_exstyle|=WS_EX_TRANSPARENT;break;
   case 14:active=0;break;
   case 15:active=(HWND)999;break;
   case 16:owner=(HWND)777;break;
   case 17:flags=GUI_INMENUMODE;break;
   case 18:q.native_text_context.serial=16;break;
   case 19:serial_current=0;break;
   case 20:gui_change=1;break;
   case 21:gui_change=2;break;
   case 22:gui_change=3;break;
   case 23:gui_change=4;break;
   case 24:gui_change=5;break;
   case 25:gui_change=6;break;
   case 26:gui_change=7;break;
   case 27:foreground_changes=1;break;
   case 28:gui_change=8;break;
   case 29:gui_change=9;break;
   case 30:gui_change=10;break;
   case 31:gui_change=11;break;
   case 32:gui_change=12;break;
  }
  macdrv_native_text_context((HWND)123,&q);
  assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_REFUSED&&cancel_calls==0&&end_calls==0);
 }
 /* Chromium toggles WS_EX_LAYERED while shown; without it this is still its menu. */
 q=captured();popup_exstyle&=~WS_EX_LAYERED;requested(&q);
 q=captured();post_ok=0;macdrv_native_text_context((HWND)123,&q);assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_REFUSED&&!q.native_text_context.popup&&cancel_calls==1);
 /* Delayed cancellation cannot migrate to a replacement popup or same-root field. */
 for(int i=0;i<12;i++){
  q=captured();requested(&q);switch(i){
   case 0:capture=(HWND)889;break;
   case 1:capture=(HWND)321;break;
   case 2:focus=(HWND)457;break;
   case 3:active=(HWND)999;break;
   case 4:flags|=GUI_INMENUMODE;break;
   case 5:owner=(HWND)777;break;
   case 6:q.native_text_context.menu_seen=0;break;
   case 7:popup_class_mode=1;break;
   case 8:popup_owner=(HWND)999;break;
   case 9:popup_thread=12;break;
   case 10:serial_current=0;break;
   case 11:popup_exstyle|=WS_EX_TRANSPARENT;break;
  }
  macdrv_native_text_context((HWND)123,&q);
  assert(q.native_text_context.phase==NATIVE_TEXT_CONTEXT_REFUSED&&cancel_calls==1&&end_calls==0);
 }
 puts("captured Chromium popup, optional layered style, asynchronous cancellation, delayed unwind and 53 guarded outcomes passed");return 0;
}
"""


class NativeTextEditingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.driver = SOURCE / "dlls/winemac.drv"
        cls.cocoa = (cls.driver / "cocoa_app.m").read_text()
        cls.header = (cls.driver / "macdrv_cocoa.h").read_text()
        cls.events = (cls.driver / "event.c").read_text()
        cls.keyboard = (cls.driver / "keyboard.c").read_text()

    def test_real_keyboard_handler_balances_modifiers_and_refuses_stale_target(self):
        start = self.keyboard.index("void macdrv_native_text_edit(")
        end = self.keyboard.index("void macdrv_hotkey_press(", start)
        with tempfile.TemporaryDirectory(prefix="native-edit-check-", dir=REPO / ".build") as directory:
            code = Path(directory) / "keyboard.c"
            binary = Path(directory) / "keyboard-test"
            server = (SOURCE / "server/queue.c").read_text()
            server_start = server.index("switch (vkey)", server.index("static int queue_keyboard_message"))
            server_end = server.index("\n    message_code =", server_start)
            normalize = "\nstatic WORD normalize_key(WORD vkey,unsigned int flags) {\n"
            normalize += "struct {struct {unsigned int vkey,flags;} kbd;} value={.kbd={vkey,flags}},*input=&value;\n"
            normalize += "int unicode=0; WORD hook_vkey=vkey;\n" + server[server_start:server_end]
            normalize += "\n(void)hook_vkey; return vkey;}\n"
            code.write_text(C_STUB + normalize + self.keyboard[start:end] + C_MAIN)
            subprocess.run(["/usr/bin/clang", "-std=c11", "-Wall", "-Wextra", "-Werror", str(code), "-o", str(binary)], check=True)
            result = subprocess.run([str(binary)], text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("2048 modifier/action cases, 16", result.stdout)

    def test_real_menu_query_checks_owner_and_waits_for_confirmed_exit(self):
        start = self.keyboard.index("static BOOL caskrun_native_text_popup(")
        end = self.keyboard.index("void macdrv_native_text_edit(", start)
        with tempfile.TemporaryDirectory(prefix="native-menu-check-", dir=REPO / ".build") as directory:
            code = Path(directory) / "menu.c"
            binary = Path(directory) / "menu-test"
            code.write_text(QUERY_STUB + self.keyboard[start:end] + QUERY_MAIN)
            subprocess.run(["/usr/bin/clang", "-std=c11", "-Wall", "-Wextra", "-Werror", str(code), "-o", str(binary)], check=True)
            result = subprocess.run([str(binary)], text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("27 refusal/fallback cases passed", result.stdout)
            self.assertIn("53 guarded outcomes passed", result.stdout)

    def test_real_cocoa_methods_scope_pair_and_target_order(self):
        start = self.cocoa.index("    - (BOOL) caskrunTextEditingEnabled")
        end = self.cocoa.index("    - (void) transformProcessToForeground:", start)
        events = (self.driver / "cocoa_event.m").read_text()
        queue_start = events.index("    - (BOOL) hasMouseButtonEvents")
        queue_end = events.index("    - (void) discardEventsPassingTest:", queue_start)
        template = (REPO / "scripts/fixtures/native-text-editing-cocoa.m.in").read_text()
        with tempfile.TemporaryDirectory(prefix="native-cocoa-check-", dir=REPO / ".build") as directory:
            code = Path(directory) / "cocoa.m"
            binary = Path(directory) / "cocoa-test"
            code.write_text(template.replace("@@PRODUCTION_METHODS@@", self.cocoa[start:end])
                            .replace("@@QUEUE_PEEK@@", events[queue_start:queue_end])
                            .replace("@@CONTEXT_GLOBALS@@", self.cocoa[self.cocoa.index("static atomic_uint caskrunTextContextSerial"):self.cocoa.index("#if !defined(MAC_OS_VERSION_14_0)")]))
            subprocess.run(["/usr/bin/clang", "-Wall", "-Wextra", "-Werror", "-framework", "AppKit", str(code), "-o", str(binary)], check=True)
            result = subprocess.run([str(binary)], text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("14 context/action refusals passed", result.stdout)
            self.assertIn("queue peek preserves events", result.stdout)
            self.assertIn("captured popup/focus query carry, delayed exit, new input and non-text scope passed", result.stdout)

    def test_exact_query_state_layout_on_unix_pe32_and_pe64(self):
        end = self.header.index("}                                           native_text_context;")
        start = self.header.rindex("struct {", 0, end) + len("struct {")
        state = self.header[start:end]
        code = "#include <stdbool.h>\n#include <stddef.h>\ntypedef struct {\n" + state + "} Context;\n"
        code += r'''
_Static_assert(sizeof(unsigned int) == 4 && sizeof(bool) == 1, "protocol scalar widths");
_Static_assert(offsetof(Context, foreground) == 0, "original foreground");
_Static_assert(offsetof(Context, focus) == sizeof(void *), "saved GUI focus");
_Static_assert(offsetof(Context, popup) == 2 * sizeof(void *), "exact captured popup");
_Static_assert(offsetof(Context, serial) == 3 * sizeof(void *), "serial offset");
_Static_assert(offsetof(Context, phase) == 3 * sizeof(void *) + 4, "phase offset");
_Static_assert(offsetof(Context, menu_seen) == 3 * sizeof(void *) + 8, "dismissal state offset");
_Static_assert(offsetof(Context, slot_owned) == 3 * sizeof(void *) + 9, "query slot offset");
_Static_assert(sizeof(Context) == (sizeof(void *) == 8 ? 40 : 24), "complete context state size");
'''
        with tempfile.TemporaryDirectory(prefix="native-context-layout-", dir=REPO / ".build") as directory:
            source = Path(directory) / "layout.c"
            source.write_text(code)
            for target in ("x86_64-apple-macos10.15", "i686-w64-windows-gnu", "x86_64-w64-windows-gnu"):
                with self.subTest(target=target):
                    subprocess.run(["/usr/bin/clang", "-target", target, "-std=c11", "-Wall", "-Wextra",
                                    "-Werror", "-fsyntax-only", str(source)], check=True)

    def test_appended_event_preserves_previous_ordinals(self):
        self.assertIn("WINDOW_RESTORE_REQUESTED,\n    NATIVE_TEXT_EDIT_ACTION,", self.header)
        self.assertIn('"WINDOW_RESTORE_REQUESTED",\n        "NATIVE_TEXT_EDIT_ACTION",', self.events)
        self.assertIn("event_mask_for_type(NATIVE_TEXT_EDIT_ACTION)", self.events)
        self.assertIn("macdrv_native_text_edit(hwnd, event)", self.events)
        self.assertIn("QUERY_MIN_MAX_INFO,\n    QUERY_NATIVE_TEXT_CONTEXT,", self.header)
        self.assertIn("macdrv_native_text_context(hwnd, query)", self.events)
        self.assertIn("macdrv_cancel_native_text_context();", (self.driver / "cocoa_window.m").read_text())

    def test_context_scope_and_balanced_targeting_pair(self):
        start = self.cocoa.index("- (BOOL) caskrunHandleTextContextEvent:")
        end = self.cocoa.index("- (void) transformProcessToForeground:", start)
        context = self.cocoa[start:end]
        for guard in ["CaskrunSteamDockEnabled()", 'CaskrunIsWindowsProcess(@"steamwebhelper.exe")',
                      "mouseCaptureWindow", "self.clippingCursor", "[windowsBeingDragged count]",
                      "[NSCursor IBeamCursor]", "window != NSApp.keyWindow", "window.closing",
                      "NSPointInRect(point, contentRect)"]:
            self.assertIn(guard, context)
        self.assertLess(context.index("CGEventGetLocation(mouseEvent)"), context.index("[self caskrunPrepareTextContextForWindow"))
        self.assertIn("for (i = 0; i < 2; i++)", context)
        self.assertIn("click->mouse_button.pressed = i == 0", context)
        self.assertIn("click->mouse_button.x = floor(targetPoint.x)", context)
        self.assertIn("click->mouse_button.y = floor(targetPoint.y)", context)
        self.assertNotIn("mouseEventWithType:", context)
        self.assertIn("NSEventTypeRightMouseUp && caskrunConsumedRightButton", context)
        self.assertIn("NSEventTypeRightMouseDragged && caskrunConsumedRightButton", context)
        self.assertIn("caskrunConsumedRightButton = NO;", context)

    def test_clipboard_contents_never_enter_bridge(self):
        start = self.cocoa.index("- (BOOL) caskrunTextEditingEnabled")
        end = self.cocoa.index("- (void) transformProcessToForeground:", start)
        bridge = self.cocoa[start:end]
        for forbidden in ["NSPasteboard", "stringForType", "readObjectsForClasses", "WM_GETTEXT", "GetWindowText"]:
            self.assertNotIn(forbidden, bridge)
        for key in ["'X'", "'C'", "'V'", "'A'"]:
            self.assertIn(key, bridge)
        self.assertIn("NSEventModifierFlagCommand", bridge)
        self.assertIn("item.representedObject = window", bridge)
        start = self.keyboard.index("static BOOL caskrun_native_text_popup(")
        end = self.keyboard.index("void macdrv_native_text_edit(", start)
        query = self.keyboard[start:end]
        self.assertIn("NtUserPostMessage(popup, WM_CANCELMODE, 0, 0)", query)
        for forbidden in ("send_message(", "VK_ESCAPE", "WM_GETTEXT", "NtUserGetWindowText",
                          "AccessibleObject", "WM_GETOBJECT", "NtUserPostMessage(popup, WM_CLOSE"):
            self.assertNotIn(forbidden, query)
        self.assertIn("macdrv_release_event(event)", bridge)


if __name__ == "__main__":
    unittest.main(argv=[__file__] + TEST_ARGS)
