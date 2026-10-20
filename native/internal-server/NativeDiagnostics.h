#pragma once
#include <Windows.h>
#include <cstdio>
#include <cstdint>

// Bounded, address-only diagnostics: never serialize commands, credentials or player data.
inline volatile LONG DR_TickStage = 0;
inline wchar_t DR_DiagnosticPath[MAX_PATH] = {};
inline volatile LONG DR_DiagnosticWrites = 0;
inline void DR_WriteDiagnostic(const char* line, DWORD length) {
    if (!DR_DiagnosticPath[0]) return;
    HANDLE file = CreateFileW(DR_DiagnosticPath, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
        nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file != INVALID_HANDLE_VALUE) {
        DWORD written = 0; WriteFile(file, line, length, &written, nullptr); CloseHandle(file);
    }
}
inline LONG CALLBACK DR_RecordException(EXCEPTION_POINTERS* info) {
    const DWORD code = info->ExceptionRecord->ExceptionCode;
    if (code != EXCEPTION_ACCESS_VIOLATION && code != 0xC0000409 && code != 0xC0000374)
        return EXCEPTION_CONTINUE_SEARCH;
    if (InterlockedIncrement(&DR_DiagnosticWrites) > 32) return EXCEPTION_CONTINUE_SEARCH;
    HMODULE module = nullptr; char name[MAX_PATH] = {}; char line[1024] = {};
    auto address = info->ExceptionRecord->ExceptionAddress;
    GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCSTR>(address), &module);
    if (module) GetModuleFileNameA(module, name, MAX_PATH);
    int size = sprintf_s(line, "first_chance code=%08lx address=%p module=%s offset=%llx tick_stage=%ld thread=%lu uptime_ms=%llu\n",
        code, address, name, static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(address) - reinterpret_cast<uintptr_t>(module)),
        DR_TickStage, GetCurrentThreadId(), GetTickCount64());
    if (size > 0) DR_WriteDiagnostic(line, static_cast<DWORD>(size));
    return EXCEPTION_CONTINUE_SEARCH;
}
inline void DR_InitDiagnostics() {
    wchar_t folder[MAX_PATH] = {};
    DWORD size = GetEnvironmentVariableW(L"GAMESERVER_READY_DIR", folder, MAX_PATH);
    if (!size || size >= MAX_PATH - 48) return;
    swprintf_s(DR_DiagnosticPath, L"%s\\native-fault-%lu.log", folder, GetCurrentProcessId());
    AddVectoredExceptionHandler(1, DR_RecordException);
}
