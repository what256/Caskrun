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
typedef struct { unsigned int cbSize,flags; HWND hwndFocus; } GUITHREADINFO;
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
            result = subprocess.run([str(binary)], check=True, text=True, capture_output=True)
            self.assertIn("2048 modifier/action cases, 16", result.stdout)

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
                            .replace("@@QUEUE_PEEK@@", events[queue_start:queue_end]))
            subprocess.run(["/usr/bin/clang", "-Wall", "-Wextra", "-Werror", "-framework", "AppKit", str(code), "-o", str(binary)], check=True)
            result = subprocess.run([str(binary)], check=True, text=True, capture_output=True)
            self.assertIn("14 context/action refusals passed", result.stdout)
            self.assertIn("queue peek preserves events", result.stdout)

    def test_appended_event_preserves_previous_ordinals(self):
        self.assertIn("WINDOW_RESTORE_REQUESTED,\n    NATIVE_TEXT_EDIT_ACTION,", self.header)
        self.assertIn('"WINDOW_RESTORE_REQUESTED",\n        "NATIVE_TEXT_EDIT_ACTION",', self.events)
        self.assertIn("event_mask_for_type(NATIVE_TEXT_EDIT_ACTION)", self.events)
        self.assertIn("macdrv_native_text_edit(hwnd, event)", self.events)

    def test_context_scope_and_balanced_targeting_pair(self):
        start = self.cocoa.index("- (BOOL) caskrunHandleTextContextEvent:")
        end = self.cocoa.index("- (void) transformProcessToForeground:", start)
        context = self.cocoa[start:end]
        for guard in ["CaskrunSteamDockEnabled()", 'CaskrunIsWindowsProcess(@"steamwebhelper.exe")',
                      "mouseCaptureWindow", "self.clippingCursor", "[windowsBeingDragged count]",
                      "[NSCursor IBeamCursor]", "window != NSApp.keyWindow", "window.closing",
                      "NSPointInRect(point, contentRect)"]:
            self.assertIn(guard, context)
        self.assertLess(context.index("CGEventGetLocation(mouseEvent)"), context.index("popUpContextMenu"))
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
        self.assertIn("macdrv_release_event(event)", bridge)


if __name__ == "__main__":
    unittest.main(argv=[__file__] + TEST_ARGS)
