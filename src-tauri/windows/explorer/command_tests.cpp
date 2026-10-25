#include "command.cpp"
#include <appmodel.h>
#include <cassert>

int wmain(int count, wchar_t** arguments) {
    if (count == 2 && std::wstring(arguments[1]) == L"--verify-unpackaged") {
        UINT32 length = 0;
        return GetCurrentPackageFullName(&length, nullptr) == APPMODEL_ERROR_NO_PACKAGE ? 0 : 1;
    }
    for (const auto& value : std::vector<std::wstring>{
        L"C:\\", L"C:\\folder space\\", L"C:\\中文 & folder #1",
        L"\\\\server\\share\\folder", L"C:\\quote\"and\\\"end\\", L""
    }) {
        int count = 0;
        auto arguments = CommandLineToArgvW((quote(L"C:\\Mango Disk\\MangoDisk.exe") + L" " + quote(value)).c_str(), &count);
        assert(arguments != nullptr && count == 2);
        assert(arguments[1] == value);
        LocalFree(arguments);
    }
    auto factory = new Factory();
    assert(DllCanUnloadNow() == S_FALSE);
    IExplorerCommand* command = nullptr;
    assert(factory->CreateInstance(nullptr, IID_PPV_ARGS(&command)) == S_OK);
    factory->Release();
    GUID id{};
    assert(command->GetCanonicalName(&id) == S_OK && id == command_id);
    command->Release();
    assert(DllCanUnloadNow() == S_OK);
    wchar_t executable[32768]{};
    assert(GetModuleFileNameW(nullptr, executable, 32768) != 0);
    auto line = quote(executable) + L" --verify-unpackaged";
    PROCESS_INFORMATION process{};
    assert(launch_application(executable, line, &process) == S_OK);
    assert(WaitForSingleObject(process.hProcess, 15000) == WAIT_OBJECT_0);
    DWORD code = 1;
    assert(GetExitCodeProcess(process.hProcess, &code) && code == 0);
    CloseHandle(process.hThread); CloseHandle(process.hProcess);
    auto missing = quote(L"Z:\\nonexistent-mangodisk-validation\\MangoDisk.exe");
    assert(FAILED(launch_application(L"Z:\\nonexistent-mangodisk-validation\\MangoDisk.exe", missing, &process)));
}
