/* Original Caskrun fixture: verify Windows cwd and a relative file read. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>
#include <string.h>

int main(int argc, char **argv)
{
    char current[4096];
    char content[64] = {0};
    DWORD count = 0;
    if (argc != 2 || !GetCurrentDirectoryA(sizeof(current), current)
            || _stricmp(current, argv[1])) {
        puts("{\"cwd_matches\":false}");
        return 1;
    }
    HANDLE file = CreateFileA("cwd-sentinel.txt", GENERIC_READ,
        FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return 2;
    BOOL read = ReadFile(file, content, sizeof(content) - 1, &count, NULL);
    CloseHandle(file);
    if (!read || strcmp(content, "original-caskrun-cwd-fixture")) return 3;
    puts("{\"cwd_matches\":true,\"relative_file_matches\":true}");
    return 0;
}
