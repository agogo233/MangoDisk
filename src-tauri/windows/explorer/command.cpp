#include <windows.h>
#include <appmodel.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <atomic>
#include <string>
#include <vector>

// The menu only forwards a selected filesystem directory. Analysis always runs
// in the ordinary MangoDisk process, outside Explorer's menu-construction path.
static const CLSID command_id = {0x93e49f90, 0xcebd, 0x4ba0, {0x93, 0xba, 0x73, 0x3a, 0x3c, 0x73, 0x22, 0x14}};
static constexpr auto settings_key = L"Software\\MangoDisk\\ExplorerIntegration";
static std::atomic<long> objects{0};

static std::wstring read_text(const wchar_t* name) {
    DWORD bytes = 0;
    if (RegGetValueW(HKEY_CURRENT_USER, settings_key, name, RRF_RT_REG_SZ, nullptr, nullptr, &bytes) != ERROR_SUCCESS ||
        bytes < sizeof(wchar_t) || bytes > 65536 || bytes % sizeof(wchar_t)) return {};
    std::vector<wchar_t> value(bytes / sizeof(wchar_t));
    if (RegGetValueW(HKEY_CURRENT_USER, settings_key, name, RRF_RT_REG_SZ, nullptr, value.data(), &bytes) != ERROR_SUCCESS ||
        value.back() != L'\0') return {};
    return value.data();
}

static bool enabled() {
    DWORD value = 0, bytes = sizeof(value);
    return RegGetValueW(HKEY_CURRENT_USER, settings_key, L"Enabled", RRF_RT_REG_DWORD, nullptr, &value, &bytes) == ERROR_SUCCESS && value == 1;
}

// Quote for CommandLineToArgvW without involving cmd.exe or PowerShell. In
// particular, a directory ending in a backslash must not escape its closing quote.
static std::wstring quote(const std::wstring& value) {
    std::wstring out = L"\"";
    size_t slashes = 0;
    for (auto ch : value) {
        if (ch == L'\\') { ++slashes; continue; }
        if (ch == L'\"') { out.append(slashes * 2 + 1, L'\\'); }
        else { out.append(slashes, L'\\'); }
        slashes = 0;
        out += ch;
    }
    out.append(slashes * 2, L'\\');
    return out + L'\"';
}

static HRESULT selected_directory(IShellItemArray* items, std::wstring* path) {
    if (!items) return E_INVALIDARG;
    DWORD count = 0;
    HRESULT result = items->GetCount(&count);
    if (FAILED(result) || count != 1) return E_INVALIDARG;
    IShellItem* item = nullptr;
    result = items->GetItemAt(0, &item);
    if (FAILED(result)) return result;
    SFGAOF attributes = 0;
    result = item->GetAttributes(SFGAO_FILESYSTEM | SFGAO_FOLDER, &attributes);
    if (SUCCEEDED(result) && attributes != (SFGAO_FILESYSTEM | SFGAO_FOLDER)) result = E_INVALIDARG;
    if (SUCCEEDED(result) && path) {
        PWSTR value = nullptr;
        result = item->GetDisplayName(SIGDN_FILESYSPATH, &value);
        if (SUCCEEDED(result)) { *path = value; CoTaskMemFree(value); }
    }
    item->Release();
    return result;
}

static HRESULT launch_application(const std::wstring& executable, std::wstring& line, PROCESS_INFORMATION* process) {
    UINT32 length = 0;
    LONG identity = GetCurrentPackageFullName(&length, nullptr);
    if (identity == APPMODEL_ERROR_NO_PACKAGE) {
        STARTUPINFOW startup{}; startup.cb = sizeof(startup);
        return CreateProcessW(executable.c_str(), line.data(), nullptr, nullptr, FALSE, 0,
            nullptr, nullptr, &startup, process) ? S_OK : HRESULT_FROM_WIN32(GetLastError());
    }
    if (identity != ERROR_INSUFFICIENT_BUFFER) return HRESULT_FROM_WIN32(identity);
    DWORD shell_id = 0;
    auto shell = GetShellWindow();
    if (!shell || !GetWindowThreadProcessId(shell, &shell_id)) return HRESULT_FROM_WIN32(ERROR_NOT_FOUND);
    SIZE_T bytes = 0;
    InitializeProcThreadAttributeList(nullptr, 1, 0, &bytes);
    if (GetLastError() != ERROR_INSUFFICIENT_BUFFER) return E_FAIL;
    std::vector<BYTE> storage(bytes);
    HANDLE parent = OpenProcess(PROCESS_CREATE_PROCESS, FALSE, shell_id);
    if (!parent) return HRESULT_FROM_WIN32(GetLastError());
    STARTUPINFOEXW startup{};
    startup.StartupInfo.cb = sizeof(startup);
    startup.lpAttributeList = reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(storage.data());
    if (!InitializeProcThreadAttributeList(startup.lpAttributeList, 1, 0, &bytes)) {
        DWORD error = GetLastError(); CloseHandle(parent); return HRESULT_FROM_WIN32(error);
    }
    // Inherit the user's ordinary shell context, not the packaged surrogate's
    // token and job. Removing the menu package must not terminate the desktop.
    BOOL created = UpdateProcThreadAttribute(startup.lpAttributeList, 0, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS,
        &parent, sizeof(parent), nullptr, nullptr);
    if (created) created = CreateProcessW(executable.c_str(), line.data(), nullptr, nullptr, FALSE,
        EXTENDED_STARTUPINFO_PRESENT, nullptr, nullptr, &startup.StartupInfo, process);
    DWORD error = created ? ERROR_SUCCESS : GetLastError();
    DeleteProcThreadAttributeList(startup.lpAttributeList);
    CloseHandle(parent);
    return HRESULT_FROM_WIN32(error);
}

class Command final : public IExplorerCommand {
    std::atomic<ULONG> references{1};
public:
    Command() { ++objects; }
    ~Command() { --objects; }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id, void** object) override {
        if (!object) return E_POINTER;
        *object = nullptr;
        if (id != IID_IUnknown && id != IID_IExplorerCommand) return E_NOINTERFACE;
        *object = static_cast<IExplorerCommand*>(this); AddRef(); return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++references; }
    ULONG STDMETHODCALLTYPE Release() override { auto left = --references; if (!left) delete this; return left; }
    HRESULT STDMETHODCALLTYPE GetTitle(IShellItemArray*, PWSTR* title) override {
        auto text = read_text(L"MenuTitle");
        return text.empty() ? E_FAIL : SHStrDupW(text.c_str(), title);
    }
    HRESULT STDMETHODCALLTYPE GetIcon(IShellItemArray*, PWSTR* icon) override {
        auto executable = read_text(L"Executable");
        return executable.empty() ? E_FAIL : SHStrDupW((executable + L",0").c_str(), icon);
    }
    HRESULT STDMETHODCALLTYPE GetToolTip(IShellItemArray*, PWSTR* tip) override { *tip = nullptr; return E_NOTIMPL; }
    HRESULT STDMETHODCALLTYPE GetCanonicalName(GUID* id) override { *id = command_id; return S_OK; }
    HRESULT STDMETHODCALLTYPE GetState(IShellItemArray* items, BOOL, EXPCMDSTATE* state) override {
        *state = enabled() && SUCCEEDED(selected_directory(items, nullptr)) ? ECS_ENABLED : ECS_HIDDEN;
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE Invoke(IShellItemArray* items, IBindCtx*) override {
        if (!enabled()) return HRESULT_FROM_WIN32(ERROR_CANCELLED);
        std::wstring directory;
        HRESULT result = selected_directory(items, &directory);
        if (FAILED(result)) return result;
        auto executable = read_text(L"Executable");
        if (executable.empty()) return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);
        auto line = quote(executable) + L" --analyze-directory " + quote(directory);
        if (line.size() >= 32767) return HRESULT_FROM_WIN32(ERROR_FILENAME_EXCED_RANGE);
        PROCESS_INFORMATION process{};
        result = launch_application(executable, line, &process);
        if (FAILED(result)) return result;
        CloseHandle(process.hThread); CloseHandle(process.hProcess);
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE GetFlags(EXPCMDFLAGS* flags) override { *flags = ECF_DEFAULT; return S_OK; }
    HRESULT STDMETHODCALLTYPE EnumSubCommands(IEnumExplorerCommand** commands) override { *commands = nullptr; return E_NOTIMPL; }
};

class Factory final : public IClassFactory {
    std::atomic<ULONG> references{1};
public:
    Factory() { ++objects; }
    ~Factory() { --objects; }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id, void** object) override {
        if (!object) return E_POINTER;
        *object = nullptr;
        if (id != IID_IUnknown && id != IID_IClassFactory) return E_NOINTERFACE;
        *object = static_cast<IClassFactory*>(this); AddRef(); return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++references; }
    ULONG STDMETHODCALLTYPE Release() override { auto left = --references; if (!left) delete this; return left; }
    HRESULT STDMETHODCALLTYPE CreateInstance(IUnknown* outer, REFIID id, void** object) override {
        if (outer) return CLASS_E_NOAGGREGATION;
        auto command = new (std::nothrow) Command();
        if (!command) return E_OUTOFMEMORY;
        auto result = command->QueryInterface(id, object); command->Release(); return result;
    }
    HRESULT STDMETHODCALLTYPE LockServer(BOOL lock) override { if (lock) ++objects; else --objects; return S_OK; }
};

extern "C" HRESULT __stdcall DllGetClassObject(REFCLSID id, REFIID interface_id, void** object) {
    if (id != command_id) return CLASS_E_CLASSNOTAVAILABLE;
    auto factory = new (std::nothrow) Factory();
    if (!factory) return E_OUTOFMEMORY;
    auto result = factory->QueryInterface(interface_id, object); factory->Release(); return result;
}
extern "C" HRESULT __stdcall DllCanUnloadNow() { return objects == 0 ? S_OK : S_FALSE; }
