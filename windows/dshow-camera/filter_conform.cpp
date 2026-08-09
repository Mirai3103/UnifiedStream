//------------------------------------------------------------------------------
// filter_conform.cpp — the COM surface a capture application actually touches.
//
// `ring_conform` proves the transport; this proves the filter. They are separate programs because
// they fail for unrelated reasons and because this one must not link the transport at all: every
// check here is made through COM, from outside, exactly as a host application makes it.
//
// It exists because of a bug that every other check in this repository missed. The filter delivered
// correct frames into a graph built by hand, passed the cross-toolchain conformance test, and
// enumerated in Discord and Chrome — and opened in neither, because the output pin did not
// implement `IAMStreamConfig`. Chromium queries it, gives up when the query fails, and reports a
// device error. Building a graph with `ConnectDirect`, as a test harness naturally does, never asks.
//
// So the rule this program encodes: it is not enough that a graph *can* be built around the filter.
// The interfaces applications use to decide whether to build one must be present and answer
// consistently. A missing interface is invisible in review and silent at run time.
//
// The DLL is loaded by path rather than through the registry, so this needs no administrator and
// leaves nothing behind on the machine that runs it.
//------------------------------------------------------------------------------

#include <windows.h>

#include <dshow.h>

#include <cstdio>
#include <cstring>

namespace {

// {6D8DD393-D871-4498-A24F-4AFFEACFC106}
constexpr GUID kFilterClsid = {
    0x6d8dd393, 0xd871, 0x4498, {0xa2, 0x4f, 0x4a, 0xff, 0xea, 0xcf, 0xc1, 0x06}};

// MEDIASUBTYPE_I420, spelled from the FOURCC for the same reason `filter.h` spells it.
constexpr GUID kSubtypeI420 = {
    0x30323449, 0x0000, 0x0010, {0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71}};

int g_failures = 0;

void Check(bool condition, const char* what) {
    std::printf("  %-58s %s\n", what, condition ? "ok" : "FAILED");
    if (!condition) {
        ++g_failures;
    }
}

using DllGetClassObjectFn = HRESULT(STDAPICALLTYPE*)(REFCLSID, REFIID, void**);

void FreeMediaType(AM_MEDIA_TYPE* media_type) {
    if (media_type == nullptr) {
        return;
    }
    if (media_type->cbFormat != 0 && media_type->pbFormat != nullptr) {
        ::CoTaskMemFree(media_type->pbFormat);
    }
    if (media_type->pUnk != nullptr) {
        media_type->pUnk->Release();
    }
    ::CoTaskMemFree(media_type);
}

IPin* OutputPin(IBaseFilter* filter) {
    IEnumPins* pins = nullptr;
    if (FAILED(filter->EnumPins(&pins))) {
        return nullptr;
    }
    IPin* pin = nullptr;
    while (pins->Next(1, &pin, nullptr) == S_OK) {
        PIN_DIRECTION direction{};
        if (SUCCEEDED(pin->QueryDirection(&direction)) && direction == PINDIR_OUTPUT) {
            pins->Release();
            return pin;
        }
        pin->Release();
    }
    pins->Release();
    return nullptr;
}

/// Width and height out of a video media type, or zeroes if it is not one this filter should offer.
void Geometry(const AM_MEDIA_TYPE* media_type, LONG* width, LONG* height) {
    *width = 0;
    *height = 0;
    if (media_type == nullptr || media_type->pbFormat == nullptr ||
        media_type->cbFormat < sizeof(VIDEOINFOHEADER)) {
        return;
    }
    const auto* info = reinterpret_cast<const VIDEOINFOHEADER*>(media_type->pbFormat);
    *width = info->bmiHeader.biWidth;
    *height = info->bmiHeader.biHeight;
}

}  // namespace

int main(int argc, char** argv) {
    const char* path = argc > 1 ? argv[1] : nullptr;
    if (path == nullptr) {
        std::fprintf(stderr, "usage: filter_conform <path to the filter DLL>\n");
        return 2;
    }
    std::printf("filter_conform: %s\n", path);

    const HRESULT com = ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(com)) {
        std::fprintf(stderr, "CoInitializeEx failed: 0x%08lX\n", static_cast<unsigned long>(com));
        return 2;
    }

    const HMODULE module = ::LoadLibraryA(path);
    if (module == nullptr) {
        std::fprintf(stderr, "could not load %s: %lu\n", path, ::GetLastError());
        return 2;
    }
    auto* get_class_object =
        reinterpret_cast<DllGetClassObjectFn>(::GetProcAddress(module, "DllGetClassObject"));
    if (get_class_object == nullptr) {
        std::fprintf(stderr, "the DLL does not export DllGetClassObject\n");
        return 2;
    }

    IClassFactory* factory = nullptr;
    HRESULT hr = get_class_object(kFilterClsid, IID_IClassFactory, reinterpret_cast<void**>(&factory));
    if (FAILED(hr) || factory == nullptr) {
        std::fprintf(stderr, "DllGetClassObject: 0x%08lX\n", static_cast<unsigned long>(hr));
        return 2;
    }

    IBaseFilter* filter = nullptr;
    hr = factory->CreateInstance(nullptr, IID_IBaseFilter, reinterpret_cast<void**>(&filter));
    if (FAILED(hr) || filter == nullptr) {
        std::fprintf(stderr, "CreateInstance(IBaseFilter): 0x%08lX\n",
                     static_cast<unsigned long>(hr));
        return 2;
    }

    CLSID reported{};
    Check(SUCCEEDED(filter->GetClassID(&reported)) && ::IsEqualGUID(reported, kFilterClsid),
          "the filter reports the CLSID the desktop probes for");

    IPin* pin = OutputPin(filter);
    Check(pin != nullptr, "the filter exposes an output pin");
    if (pin == nullptr) {
        return 1;
    }

    // Capture-graph builders identify a capture pin this way and will not use one that cannot say.
    IKsPropertySet* property_set = nullptr;
    hr = pin->QueryInterface(IID_IKsPropertySet, reinterpret_cast<void**>(&property_set));
    Check(SUCCEEDED(hr) && property_set != nullptr, "the pin implements IKsPropertySet");
    if (SUCCEEDED(hr) && property_set != nullptr) {
        GUID category{};
        DWORD returned = 0;
        const HRESULT got = property_set->Get(AMPROPSETID_Pin, AMPROPERTY_PIN_CATEGORY, nullptr, 0,
                                              &category, sizeof(category), &returned);
        Check(SUCCEEDED(got) && ::IsEqualGUID(category, PIN_CATEGORY_CAPTURE),
              "the pin reports PIN_CATEGORY_CAPTURE");
        property_set->Release();
    }

    // The interface whose absence started all this. Chromium queries it before anything else and
    // abandons the device when the query fails.
    IAMStreamConfig* config = nullptr;
    hr = pin->QueryInterface(IID_IAMStreamConfig, reinterpret_cast<void**>(&config));
    Check(SUCCEEDED(hr) && config != nullptr, "the pin implements IAMStreamConfig");
    if (FAILED(hr) || config == nullptr) {
        std::printf("\n%d check(s) failed\n", g_failures);
        return 1;
    }

    int count = 0;
    int size = 0;
    hr = config->GetNumberOfCapabilities(&count, &size);
    Check(SUCCEEDED(hr) && count > 0, "the pin reports at least one capability");
    Check(size == static_cast<int>(sizeof(VIDEO_STREAM_CONFIG_CAPS)),
          "the capability structure is VIDEO_STREAM_CONFIG_CAPS");

    LONG last_width = 0;
    LONG last_height = 0;
    for (int i = 0; i < count; ++i) {
        AM_MEDIA_TYPE* media_type = nullptr;
        VIDEO_STREAM_CONFIG_CAPS caps{};
        hr = config->GetStreamCaps(i, &media_type, reinterpret_cast<BYTE*>(&caps));
        if (FAILED(hr) || media_type == nullptr) {
            Check(false, "GetStreamCaps returned a media type");
            continue;
        }

        LONG width = 0;
        LONG height = 0;
        Geometry(media_type, &width, &height);
        char label[96];

        std::snprintf(label, sizeof(label), "capability %d is I420 video at %ldx%ld", i, width,
                      height);
        Check(::IsEqualGUID(media_type->majortype, MEDIATYPE_Video) &&
                  ::IsEqualGUID(media_type->subtype, kSubtypeI420) && width > 0 && height > 0,
              label);

        // A capability list that disagrees with itself is worse than none: applications size their
        // buffers from the caps and read frames of the media type.
        std::snprintf(label, sizeof(label), "capability %d agrees with its own caps structure", i);
        Check(caps.InputSize.cx == width && caps.InputSize.cy == height &&
                  caps.MaxOutputSize.cx == width && caps.MaxOutputSize.cy == height &&
                  caps.MinFrameInterval > 0 && caps.MaxFrameInterval >= caps.MinFrameInterval,
              label);

        std::snprintf(label, sizeof(label), "capability %d declares the I420 sample size", i);
        Check(media_type->lSampleSize ==
                  static_cast<ULONG>(width) * static_cast<ULONG>(height) * 3 / 2,
              label);

        last_width = width;
        last_height = height;
        if (i == count - 1) {
            // Select the last one, which is deliberately not the default, so a SetFormat that is
            // accepted and then ignored is caught by GetFormat rather than passing silently.
            hr = config->SetFormat(media_type);
            Check(SUCCEEDED(hr), "SetFormat accepts a format the pin itself offered");
        }
        FreeMediaType(media_type);
    }

    AM_MEDIA_TYPE* current = nullptr;
    hr = config->GetFormat(&current);
    LONG current_width = 0;
    LONG current_height = 0;
    Geometry(current, &current_width, &current_height);
    Check(SUCCEEDED(hr) && current_width == last_width && current_height == last_height,
          "GetFormat returns the format SetFormat selected");
    FreeMediaType(current);

    AM_MEDIA_TYPE bogus{};
    bogus.majortype = MEDIATYPE_Audio;
    Check(config->SetFormat(&bogus) != S_OK, "SetFormat refuses a format the pin does not offer");

    config->Release();
    pin->Release();
    filter->Release();
    factory->Release();

    if (g_failures != 0) {
        std::printf("\n%d check(s) failed\n", g_failures);
        return 1;
    }
    std::printf("\nok\n");
    return 0;
}
