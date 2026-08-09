//------------------------------------------------------------------------------
// dllmain.cpp — COM entry points and registration.
//
// Registration writes exactly what the desktop's availability probe already reads. Three things,
// and the third is the one that is easy to forget and invisible when forgotten:
//
//   1. `InprocServer32` under `HKCR\CLSID\{6D8DD393-...}`, naming this DLL's own path, resolved at
//      run time from the module handle rather than assumed. The user may have installed anywhere,
//      and `platform/windows.rs` confirms the named file exists on disk before reporting the filter
//      installed.
//   2. The filter under `CLSID_VideoInputDeviceCategory` through `IFilterMapper2`, which is what
//      makes applications enumerate the camera at all.
//   3. `TransportVersion` as a `REG_DWORD` on the CLSID key. `platform/windows.rs` treats a missing
//      value as version 0 and reports `VersionMismatch`, so a filter that skips this step is a
//      filter the desktop refuses to talk to — while looking, from here, entirely correct.
//
// `DllUnregisterServer` reverses all three. A leftover CLSID leaves a camera in every application's
// device list that cannot be loaded.
//
// **One architecture per invocation.** Nothing here knows or asks which registry view it is in, and
// nothing should: a 32-bit DLL registered by the 32-bit `regsvr32` in `SysWOW64` lands in
// `WOW6432Node` naturally, which is exactly where the desktop's `probe_view(KEY_WOW64_32KEY)` looks.
// The desktop's `FilterAvailability::OneArchitecture` walks the user through the two commands one at
// a time, and registering both from one of them would break that.
//------------------------------------------------------------------------------

#include <streams.h>

#include "filter.h"
#include "transport.h"

namespace {

/// The name applications see, taken straight from `transport.h` rather than written out again. It
/// is `transport::FILTER_FRIENDLY_NAME`, which is `CAMERA_NODE_LABEL`: the camera has one name
/// across the product, and a second spelling here would quietly give it two.
///
/// The `const_cast`s exist because the DirectShow setup structures declare their name fields as
/// `LPWSTR`. Nothing writes through them.
LPWSTR const g_filter_name = const_cast<LPWSTR>(unifiedstream::kFilterFriendlyName);
LPWSTR const g_pin_name = const_cast<LPWSTR>(L"Capture");

const AMOVIESETUP_MEDIATYPE kOutputTypes = {&MEDIATYPE_Video, &MEDIASUBTYPE_I420};

const AMOVIESETUP_PIN kOutputPin = {
    g_pin_name,   // strName
    FALSE,        // bRendered
    TRUE,         // bOutput
    FALSE,        // bZero — the pin always exists
    FALSE,        // bMany — exactly one
    &CLSID_NULL,  // clsConnectsToFilter
    nullptr,      // strConnectsToPin
    1,            // nMediaTypes
    &kOutputTypes,
};

const AMOVIESETUP_FILTER kFilterSetup = {
    &CLSID_UnifiedStreamCamera,
    g_filter_name,
    MERIT_DO_NOT_USE,  // a capture device is chosen by the user, never by intelligent connect
    1,
    &kOutputPin,
};

/// A COM apartment held only for the duration of a registration call.
///
/// `regsvr32` initialises COM before calling in, so this is almost always a no-op refcount — but
/// `AMovieDllRegisterServer2` calls `CoInitialize` itself and the `IFilterMapper2` work below needs
/// an apartment whoever the caller is.
class ComScope {
public:
    ComScope() noexcept : hr_(::CoInitialize(nullptr)) {}
    ~ComScope() noexcept {
        if (SUCCEEDED(hr_)) {
            ::CoUninitialize();
        }
    }
    ComScope(const ComScope&) = delete;
    ComScope& operator=(const ComScope&) = delete;

    [[nodiscard]] bool usable() const noexcept {
        // RPC_E_CHANGED_MODE means somebody else already picked the apartment model. Their
        // apartment works perfectly well for this; it is just not ours to uninitialise.
        return SUCCEEDED(hr_) || hr_ == RPC_E_CHANGED_MODE;
    }

private:
    HRESULT hr_;
};

/// `CLSID\{...}` for this filter, derived from the binary GUID so there is one source of truth.
HRESULT ClsidKeyPath(WCHAR (&path)[64], WCHAR (&clsid_text)[CHARS_IN_GUID]) noexcept {
    if (::StringFromGUID2(CLSID_UnifiedStreamCamera, clsid_text, CHARS_IN_GUID) == 0) {
        return E_UNEXPECTED;
    }
    // The desktop probes for the CLSID as text, written out in `transport::FILTER_CLSID`. If the
    // binary GUID compiled into this filter and that text ever diverge, no compiler notices and the
    // camera silently stops being found — so it is checked here, where the answer is knowable.
    if (::lstrcmpiW(clsid_text, unifiedstream::kFilterClsidText) != 0) {
        return E_UNEXPECTED;
    }
    ::wsprintfW(path, L"CLSID\\%ls", clsid_text);
    return S_OK;
}

/// Register the filter under `CLSID_VideoInputDeviceCategory`, or remove it from there.
HRESULT SetVideoInputCategory(bool register_it) noexcept {
    IFilterMapper2* mapper = nullptr;
    HRESULT hr = ::CoCreateInstance(CLSID_FilterMapper2, nullptr, CLSCTX_INPROC_SERVER,
                                    IID_IFilterMapper2, reinterpret_cast<void**>(&mapper));
    if (FAILED(hr)) {
        return hr;
    }

    // Remove any previous registration first, so re-running `regsvr32` after moving the DLL leaves
    // one entry rather than two.
    hr = mapper->UnregisterFilter(&CLSID_VideoInputDeviceCategory, g_filter_name,
                                  CLSID_UnifiedStreamCamera);
    if (hr == HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)) {
        hr = S_OK;
    }

    if (register_it && SUCCEEDED(hr)) {
        REGPINTYPES types{};
        types.clsMajorType = &MEDIATYPE_Video;
        types.clsMinorType = &MEDIASUBTYPE_I420;

        REGFILTERPINS pin{};
        pin.strName = g_pin_name;
        pin.bRendered = FALSE;
        pin.bOutput = TRUE;
        pin.bZero = FALSE;
        pin.bMany = FALSE;
        pin.clsConnectsToFilter = &CLSID_NULL;
        pin.strConnectsToPin = nullptr;
        pin.nMediaTypes = 1;
        pin.lpMediaType = &types;

        REGFILTER2 filter{};
        filter.dwVersion = 1;
        filter.dwMerit = MERIT_DO_NOT_USE;
        filter.cPins = 1;
        filter.rgPins = &pin;

        hr = mapper->RegisterFilter(CLSID_UnifiedStreamCamera, g_filter_name, nullptr,
                                    &CLSID_VideoInputDeviceCategory, g_filter_name, &filter);
    }

    mapper->Release();
    return hr;
}

/// Write, or remove, the transport version the desktop's probe reads before allowing a stream.
HRESULT SetTransportVersion(bool write_it) noexcept {
    WCHAR clsid_text[CHARS_IN_GUID]{};
    WCHAR path[64]{};
    HRESULT hr = ClsidKeyPath(path, clsid_text);
    if (FAILED(hr)) {
        return hr;
    }

    HKEY key = nullptr;
    const LONG opened = ::RegOpenKeyExW(HKEY_CLASSES_ROOT, path, 0, KEY_SET_VALUE, &key);
    if (opened != ERROR_SUCCESS) {
        // On removal the coclass key may already be gone, which is the desired end state.
        return write_it ? HRESULT_FROM_WIN32(opened) : S_OK;
    }

    LONG result = ERROR_SUCCESS;
    if (write_it) {
        const DWORD version = unifiedstream::kTransportVersion;
        result = ::RegSetValueExW(key, unifiedstream::kFilterVersionValue, 0, REG_DWORD,
                                  reinterpret_cast<const BYTE*>(&version), sizeof(version));
    } else {
        result = ::RegDeleteValueW(key, unifiedstream::kFilterVersionValue);
        if (result == ERROR_FILE_NOT_FOUND) {
            result = ERROR_SUCCESS;
        }
    }
    ::RegCloseKey(key);
    return result == ERROR_SUCCESS ? S_OK : HRESULT_FROM_WIN32(result);
}

}  // namespace

/// The one coclass this DLL serves.
CFactoryTemplate g_Templates[] = {
    {g_filter_name, &CLSID_UnifiedStreamCamera, CUnifiedStreamCamera::CreateInstance, nullptr,
     &kFilterSetup},
};
int g_cTemplates = sizeof(g_Templates) / sizeof(g_Templates[0]);

STDAPI DllRegisterServer() {
    ComScope com;
    if (!com.usable()) {
        return E_FAIL;
    }

    // Step 1: the coclass and `InprocServer32`, with this DLL's own path taken from the module
    // handle. Also registers the filter under the legacy category, which is what makes it visible
    // in graph-editing tools; harmless, and reversed below.
    HRESULT hr = AMovieDllRegisterServer2(TRUE);
    if (FAILED(hr)) {
        return hr;
    }

    // Step 2: the video input device category, which is the registration applications enumerate.
    hr = SetVideoInputCategory(true);
    if (FAILED(hr)) {
        AMovieDllRegisterServer2(FALSE);
        return hr;
    }

    // Step 3: the declared transport version. Without it the desktop refuses the stream, so a
    // partial registration is rolled back rather than left looking installed.
    hr = SetTransportVersion(true);
    if (FAILED(hr)) {
        SetVideoInputCategory(false);
        AMovieDllRegisterServer2(FALSE);
        return hr;
    }

    return S_OK;
}

STDAPI DllUnregisterServer() {
    ComScope com;
    if (!com.usable()) {
        return E_FAIL;
    }

    // In reverse, and each step's failure recorded rather than allowed to skip the rest: a partial
    // removal is the state that leaves a camera in every application's device list that cannot be
    // loaded, so every step runs even if an earlier one failed.
    const HRESULT version_hr = SetTransportVersion(false);
    const HRESULT category_hr = SetVideoInputCategory(false);
    // Eliminates the whole `CLSID\{...}` subtree, so nothing this registration created survives.
    const HRESULT server_hr = AMovieDllRegisterServer2(FALSE);

    if (FAILED(version_hr)) {
        return version_hr;
    }
    if (FAILED(category_hr)) {
        return category_hr;
    }
    return server_hr;
}

extern "C" BOOL WINAPI DllEntryPoint(HINSTANCE instance, ULONG reason, LPVOID reserved);

BOOL APIENTRY DllMain(HANDLE module, DWORD reason, LPVOID reserved) {
    return DllEntryPoint(static_cast<HINSTANCE>(module), reason, reserved);
}
