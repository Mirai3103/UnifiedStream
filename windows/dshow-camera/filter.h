//------------------------------------------------------------------------------
// filter.h — the DirectShow source filter itself.
//
// A `CSource` with one `CSourceStream` output pin, registered under
// `CLSID_VideoInputDeviceCategory` so applications enumerate it as a camera.
//
// The three facts that shape everything here, from the change design:
//
// - **This runs in someone else's process.** Zoom, Chrome, Discord, and OBS load this DLL into
//   their own address space. A fault is their crash; a hang is their hang. So there is no
//   allocation on the streaming path once connected, no waiting path anywhere, and no decoder.
// - **It is usually loaded before the producer exists.** The user opens their conferencing
//   application, picks the camera, and only then reaches for their phone. The media types below are
//   therefore fixed and answered without consulting the ring, which very often does not exist at
//   the moment the graph connects.
// - **`FillBuffer` always returns a frame.** Every producer state maps onto a source of pixels; a
//   filter that stops delivering does not show a frozen picture, it hangs its host.
//------------------------------------------------------------------------------

#ifndef UNIFIEDSTREAM_DSHOW_FILTER_H
#define UNIFIEDSTREAM_DSHOW_FILTER_H

#include <streams.h>

#include <cstdint>
#include <vector>

#include "section.h"
#include "transport.h"

/// `transport::FILTER_CLSID`. Permanent: the desktop's availability probe looks for this exact
/// value in both registry views, and a mismatch is caught by neither build.
// {6D8DD393-D871-4498-A24F-4AFFEACFC106}
extern "C" const GUID CLSID_UnifiedStreamCamera;

/// `MEDIASUBTYPE_I420`. Not in `uuids.h` — the SDK ships `MEDIASUBTYPE_IYUV`, which is the same
/// layout under the other of its two FOURCCs — so it is spelled out here from the FOURCC the
/// transport pins, `transport::FOURCC_I420`.
extern "C" const GUID MEDIASUBTYPE_I420;

class CUnifiedStreamCameraStream;

/// The filter. Holds the pin and nothing else; all the behaviour is on the stream.
class CUnifiedStreamCamera : public CSource {
public:
    /// The entry point `g_Templates` names.
    static CUnknown* WINAPI CreateInstance(LPUNKNOWN unknown, HRESULT* hr);

    DECLARE_IUNKNOWN;

private:
    CUnifiedStreamCamera(LPUNKNOWN unknown, HRESULT* hr);
};

/// The one output pin, and the whole of the streaming behaviour.
class CUnifiedStreamCameraStream : public CSourceStream,
                                   public IKsPropertySet,
                                   public IAMStreamConfig {
public:
    CUnifiedStreamCameraStream(HRESULT* hr, CUnifiedStreamCamera* filter);
    ~CUnifiedStreamCameraStream() override;

    DECLARE_IUNKNOWN;
    STDMETHODIMP NonDelegatingQueryInterface(REFIID riid, void** ppv) override;

    // --- IKsPropertySet -----------------------------------------------------
    //
    // Only `AMPROPERTY_PIN_CATEGORY`, answering `PIN_CATEGORY_CAPTURE`. Capture-graph builders and
    // several of the applications this filter exists for identify a capture pin this way and will
    // not use a pin that cannot answer.
    STDMETHODIMP Set(REFGUID set, DWORD id, LPVOID instance, DWORD instance_bytes, LPVOID property,
                     DWORD property_bytes) override;
    STDMETHODIMP Get(REFGUID set, DWORD id, LPVOID instance, DWORD instance_bytes, LPVOID property,
                     DWORD property_bytes, DWORD* returned) override;
    STDMETHODIMP QuerySupported(REFGUID set, DWORD id, DWORD* support) override;

    // --- IAMStreamConfig ----------------------------------------------------
    //
    // How a capture application discovers what a camera can do. Enumerating the pin's media types
    // is the other way, and it is the way this filter was originally written for — but it is not
    // the way the applications this filter exists for actually ask.
    //
    // Chromium's `VideoCaptureDeviceWin` queries this interface on the capture pin and abandons the
    // device when the query fails, which is every Electron application and every Chromium browser:
    // the camera still *enumerates*, so it appears in the device list, and then never opens. The
    // symptom is a black picture or `NotReadableError`, with nothing logged anywhere.
    STDMETHODIMP SetFormat(AM_MEDIA_TYPE* media_type) override;
    STDMETHODIMP GetFormat(AM_MEDIA_TYPE** media_type) override;
    STDMETHODIMP GetNumberOfCapabilities(int* count, int* size) override;
    STDMETHODIMP GetStreamCaps(int index, AM_MEDIA_TYPE** media_type, BYTE* caps) override;

protected:
    // --- CSourceStream / CBaseOutputPin / CBasePin --------------------------
    HRESULT GetMediaType(int position, CMediaType* media_type) override;
    HRESULT CheckMediaType(const CMediaType* media_type) override;
    HRESULT DecideBufferSize(IMemAllocator* allocator, ALLOCATOR_PROPERTIES* request) override;
    HRESULT SetMediaType(const CMediaType* media_type) override;
    HRESULT FillBuffer(IMediaSample* sample) override;
    HRESULT OnThreadCreate() override;
    HRESULT OnThreadDestroy() override;

private:
    /// One of the geometries this filter offers, without ever asking the ring.
    struct Geometry {
        std::uint32_t width;
        std::uint32_t height;
    };

    /// Try to attach if detached and the retry timer has expired. Bounded, never waits, and never
    /// touches the network or the disk.
    void MaintainAttachment() noexcept;

    /// Sleep out the remainder of this frame's interval. Bounded by the interval itself and
    /// independent of the producer: this paces the graph, it does not wait for the desktop.
    void PaceFrame() noexcept;

    /// Compute the sample's start and end from the ring timestamp, kept monotonic in stream time.
    void TimeSample(bool have_new_frame, std::uint64_t timestamp_us, REFERENCE_TIME* start,
                    REFERENCE_TIME* end) noexcept;

    /// Fill `media_type` with the fixed description of one offered geometry. The single place a
    /// media type is built, so `GetMediaType` and `GetStreamCaps` cannot describe the same
    /// geometry differently.
    static HRESULT BuildMediaType(std::uint32_t width, std::uint32_t height,
                                  CMediaType* media_type);

    /// The geometry `GetMediaType` should offer at `position`, which is the selected one first.
    /// Connection takes the first type the peer accepts, so this is what makes `SetFormat` mean
    /// anything.
    [[nodiscard]] int GeometryIndexAt(int position) const noexcept;

    /// Index into `kOfferedGeometries` of the format an application asked for through
    /// `IAMStreamConfig::SetFormat`, or the default until one does.
    int selected_ = 0;

    // Geometry the graph connected at, fixed for the life of the connection.
    std::uint32_t width_ = 0;
    std::uint32_t height_ = 0;
    std::size_t frame_bytes_ = 0;
    REFERENCE_TIME frame_length_ = 0;
    std::uint64_t frame_interval_ms_ = 0;

    // Everything the streaming path touches, sized once at connection time. `std::vector` is used
    // for the allocation and for nothing else — no element is ever appended on the streaming path.
    std::vector<std::uint8_t> ring_frame_;   // one maximum-geometry ring payload
    std::vector<std::uint8_t> held_frame_;   // the last picture delivered, at connected geometry
    std::vector<std::uint8_t> placeholder_;  // rendered once, not per frame

    bool have_held_frame_ = false;

    unifiedstream::CameraSection section_;
    unifiedstream::RingConsumer consumer_;
    /// Set when the mapping turned out to speak a transport version this filter does not. Terminal
    /// for the process: retrying can only fail identically, and the desktop reports the
    /// disagreement from the registry before a stream is allowed to start.
    bool version_disagreement_ = false;
    std::uint64_t next_attach_ms_ = 0;
    std::uint64_t ring_generation_ = 0;

    // Pacing and timestamps.
    std::uint64_t next_due_ms_ = 0;
    REFERENCE_TIME last_end_ = 0;
    bool have_time_base_ = false;
    REFERENCE_TIME time_base_rt_ = 0;
    std::uint64_t time_base_us_ = 0;
    bool first_sample_ = true;
};

#endif  // UNIFIEDSTREAM_DSHOW_FILTER_H
