//------------------------------------------------------------------------------
// filter.cpp — see filter.h.
//------------------------------------------------------------------------------

#include "filter.h"

#include <algorithm>
#include <cstring>

#include "video.h"

using unifiedstream::AttachStatus;
using unifiedstream::FrameInfo;
using unifiedstream::I420Bytes;
using unifiedstream::Liveness;
using unifiedstream::ReadOutcome;
using unifiedstream::TickMs;

// {6D8DD393-D871-4498-A24F-4AFFEACFC106}
extern "C" const GUID CLSID_UnifiedStreamCamera = {
    0x6d8dd393, 0xd871, 0x4498, {0xa2, 0x4f, 0x4a, 0xff, 0xea, 0xcf, 0xc1, 0x06}};

// 'I420' as a DirectShow media subtype: the FOURCC in the first four bytes, then the standard
// MEDIASUBTYPE suffix `0000-0010-8000-00AA00389B71`.
extern "C" const GUID MEDIASUBTYPE_I420 = {
    0x30323449, 0x0000, 0x0010, {0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71}};

namespace {

/// The geometries this filter offers. Fixed, and answered without consulting the ring: at connect
/// time there is usually no producer, and a filter that cannot answer until the phone is streaming
/// fails to connect in the ordinary case where the user opens their conferencing application first.
///
/// Whatever the ring turns out to be publishing is scaled into whichever of these the graph picked,
/// so a resolution change on the phone never reaches the graph as a format change.
constexpr struct {
    std::uint32_t width;
    std::uint32_t height;
} kOfferedGeometries[] = {
    {1280, 720},
    {1920, 1080},
    {640, 480},
};
constexpr int kOfferedCount = static_cast<int>(std::size(kOfferedGeometries));

/// 30 fps, in 100-nanosecond units. The only rate offered.
constexpr REFERENCE_TIME kFrameLength = 333333;

/// Roughly twice a second, per the change design. Retrying every frame would hammer the object
/// manager for no benefit; half a second is indistinguishable from instant to the user starting a
/// stream.
constexpr std::uint64_t kAttachRetryMs = 500;

}  // namespace

// ---------------------------------------------------------------------------
// CUnifiedStreamCamera
// ---------------------------------------------------------------------------

CUnifiedStreamCamera::CUnifiedStreamCamera(LPUNKNOWN unknown, HRESULT* hr)
    : CSource(NAME("UnifiedStream Camera"), unknown, CLSID_UnifiedStreamCamera, hr) {
    // The pin registers itself with this filter in its own constructor, and `CSource`'s destructor
    // deletes it. Nothing to hold here.
    new CUnifiedStreamCameraStream(hr, this);
}

CUnknown* WINAPI CUnifiedStreamCamera::CreateInstance(LPUNKNOWN unknown, HRESULT* hr) {
    auto* filter = new CUnifiedStreamCamera(unknown, hr);
    if (filter == nullptr && hr != nullptr) {
        *hr = E_OUTOFMEMORY;
    }
    return filter;
}

// ---------------------------------------------------------------------------
// CUnifiedStreamCameraStream — construction and interfaces
// ---------------------------------------------------------------------------

CUnifiedStreamCameraStream::CUnifiedStreamCameraStream(HRESULT* hr, CUnifiedStreamCamera* filter)
    : CSourceStream(NAME("UnifiedStream Camera Stream"), hr, filter, L"Capture") {}

CUnifiedStreamCameraStream::~CUnifiedStreamCameraStream() = default;

STDMETHODIMP CUnifiedStreamCameraStream::NonDelegatingQueryInterface(REFIID riid, void** ppv) {
    if (riid == IID_IKsPropertySet) {
        return GetInterface(static_cast<IKsPropertySet*>(this), ppv);
    }
    if (riid == IID_IAMStreamConfig) {
        return GetInterface(static_cast<IAMStreamConfig*>(this), ppv);
    }
    return CSourceStream::NonDelegatingQueryInterface(riid, ppv);
}

STDMETHODIMP CUnifiedStreamCameraStream::Set(REFGUID, DWORD, LPVOID, DWORD, LPVOID, DWORD) {
    // Nothing about this pin is settable. Read-only is the whole shape of this component.
    return E_NOTIMPL;
}

STDMETHODIMP CUnifiedStreamCameraStream::Get(REFGUID set, DWORD id, LPVOID, DWORD, LPVOID property,
                                             DWORD property_bytes, DWORD* returned) {
    if (set != AMPROPSETID_Pin) {
        return E_PROP_SET_UNSUPPORTED;
    }
    if (id != AMPROPERTY_PIN_CATEGORY) {
        return E_PROP_ID_UNSUPPORTED;
    }
    if (returned != nullptr) {
        *returned = sizeof(GUID);
    }
    if (property_bytes == 0) {
        // A size query. Callers make one of these before allocating.
        return S_OK;
    }
    if (property == nullptr) {
        return E_POINTER;
    }
    if (property_bytes < sizeof(GUID)) {
        return E_UNEXPECTED;
    }
    *static_cast<GUID*>(property) = PIN_CATEGORY_CAPTURE;
    return S_OK;
}

STDMETHODIMP CUnifiedStreamCameraStream::QuerySupported(REFGUID set, DWORD id, DWORD* support) {
    if (set != AMPROPSETID_Pin) {
        return E_PROP_SET_UNSUPPORTED;
    }
    if (id != AMPROPERTY_PIN_CATEGORY) {
        return E_PROP_ID_UNSUPPORTED;
    }
    if (support != nullptr) {
        *support = KSPROPERTY_SUPPORT_GET;
    }
    return S_OK;
}

// ---------------------------------------------------------------------------
// Media types — answered without consulting the ring
// ---------------------------------------------------------------------------

int CUnifiedStreamCameraStream::GeometryIndexAt(int position) const noexcept {
    // The selected geometry first, then the rest in their fixed order. Connection walks this list
    // and takes the first type the peer accepts, so putting the selection at the front is what
    // makes `SetFormat` decide the resolution rather than merely record a preference.
    if (position == 0) {
        return selected_;
    }
    return position <= selected_ ? position - 1 : position;
}

HRESULT CUnifiedStreamCameraStream::BuildMediaType(std::uint32_t width, std::uint32_t height,
                                                   CMediaType* media_type) {
    CheckPointer(media_type, E_POINTER);
    const DWORD image_bytes = static_cast<DWORD>(I420Bytes(width, height));

    auto* info = reinterpret_cast<VIDEOINFOHEADER*>(
        media_type->AllocFormatBuffer(sizeof(VIDEOINFOHEADER)));
    if (info == nullptr) {
        return E_OUTOFMEMORY;
    }
    ZeroMemory(info, sizeof(VIDEOINFOHEADER));

    info->bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info->bmiHeader.biWidth = static_cast<LONG>(width);
    // Positive: planar YUV is defined top-down, and a negative height here is how a filter tells a
    // renderer the rows of an RGB bitmap are the other way up.
    info->bmiHeader.biHeight = static_cast<LONG>(height);
    info->bmiHeader.biPlanes = 1;
    info->bmiHeader.biBitCount = 12;
    info->bmiHeader.biCompression = MAKEFOURCC('I', '4', '2', '0');
    info->bmiHeader.biSizeImage = image_bytes;
    info->AvgTimePerFrame = kFrameLength;
    info->dwBitRate = image_bytes * 8 * 30;
    SetRectEmpty(&info->rcSource);
    SetRectEmpty(&info->rcTarget);

    media_type->SetType(&MEDIATYPE_Video);
    media_type->SetFormatType(&FORMAT_VideoInfo);
    media_type->SetSubtype(&MEDIASUBTYPE_I420);
    media_type->SetTemporalCompression(FALSE);
    media_type->SetSampleSize(image_bytes);
    return S_OK;
}

HRESULT CUnifiedStreamCameraStream::GetMediaType(int position, CMediaType* media_type) {
    CheckPointer(media_type, E_POINTER);
    if (position < 0) {
        return E_INVALIDARG;
    }
    if (position >= kOfferedCount) {
        return VFW_S_NO_MORE_ITEMS;
    }
    const auto geometry = kOfferedGeometries[GeometryIndexAt(position)];
    return BuildMediaType(geometry.width, geometry.height, media_type);
}

// ---------------------------------------------------------------------------
// IAMStreamConfig — the interface capture applications actually ask with
// ---------------------------------------------------------------------------

STDMETHODIMP CUnifiedStreamCameraStream::GetNumberOfCapabilities(int* count, int* size) {
    CheckPointer(count, E_POINTER);
    CheckPointer(size, E_POINTER);
    *count = kOfferedCount;
    *size = sizeof(VIDEO_STREAM_CONFIG_CAPS);
    return S_OK;
}

STDMETHODIMP CUnifiedStreamCameraStream::GetStreamCaps(int index, AM_MEDIA_TYPE** media_type,
                                                       BYTE* caps) {
    CheckPointer(media_type, E_POINTER);
    CheckPointer(caps, E_POINTER);
    if (index < 0 || index >= kOfferedCount) {
        return S_FALSE;
    }

    // The fixed order, not the selected-first order `GetMediaType` uses: this is a capability list,
    // and an application that reads index 2 twice must get the same answer both times even if it
    // called `SetFormat` in between.
    const auto geometry = kOfferedGeometries[index];

    CMediaType built;
    const HRESULT hr = BuildMediaType(geometry.width, geometry.height, &built);
    if (FAILED(hr)) {
        return hr;
    }
    *media_type = CreateMediaType(&built);
    if (*media_type == nullptr) {
        return E_OUTOFMEMORY;
    }

    const SIZE dimensions{static_cast<LONG>(geometry.width), static_cast<LONG>(geometry.height)};
    const DWORD image_bytes = static_cast<DWORD>(I420Bytes(geometry.width, geometry.height));

    auto* config = reinterpret_cast<VIDEO_STREAM_CONFIG_CAPS*>(caps);
    ZeroMemory(config, sizeof(VIDEO_STREAM_CONFIG_CAPS));
    config->guid = FORMAT_VideoInfo;
    config->VideoStandard = AnalogVideo_None;
    config->InputSize = dimensions;
    // One fixed size per entry, and no cropping or stretching: this filter scales in software to
    // the geometry the graph connected at and offers nothing else at that index.
    config->MinCroppingSize = dimensions;
    config->MaxCroppingSize = dimensions;
    config->CropGranularityX = 1;
    config->CropGranularityY = 1;
    config->CropAlignX = 1;
    config->CropAlignY = 1;
    config->MinOutputSize = dimensions;
    config->MaxOutputSize = dimensions;
    config->OutputGranularityX = 1;
    config->OutputGranularityY = 1;
    config->MinFrameInterval = kFrameLength;
    config->MaxFrameInterval = kFrameLength;
    config->MinBitsPerSecond = static_cast<LONG>(image_bytes * 8 * 30);
    config->MaxBitsPerSecond = config->MinBitsPerSecond;
    return S_OK;
}

STDMETHODIMP CUnifiedStreamCameraStream::GetFormat(AM_MEDIA_TYPE** media_type) {
    CheckPointer(media_type, E_POINTER);
    CAutoLock lock(m_pFilter->pStateLock());

    // Once connected the answer is the connection, which is the only thing the caller can act on.
    // Before that it is the selection, which is what the connection will be.
    if (IsConnected()) {
        *media_type = CreateMediaType(&m_mt);
        return *media_type == nullptr ? E_OUTOFMEMORY : S_OK;
    }

    CMediaType built;
    const HRESULT hr =
        BuildMediaType(kOfferedGeometries[selected_].width, kOfferedGeometries[selected_].height,
                       &built);
    if (FAILED(hr)) {
        return hr;
    }
    *media_type = CreateMediaType(&built);
    return *media_type == nullptr ? E_OUTOFMEMORY : S_OK;
}

STDMETHODIMP CUnifiedStreamCameraStream::SetFormat(AM_MEDIA_TYPE* media_type) {
    CheckPointer(media_type, E_POINTER);
    CAutoLock lock(m_pFilter->pStateLock());

    // Changing the geometry mid-stream would resize every buffer `SetMediaType` sized, on the
    // streaming thread, in someone else's process. Applications set the format before they run the
    // graph; one that does not is told so rather than served a reallocation.
    if (m_pFilter->IsActive()) {
        return VFW_E_WRONG_STATE;
    }

    const CMediaType requested(*media_type);
    if (CheckMediaType(&requested) != S_OK) {
        return VFW_E_INVALIDMEDIATYPE;
    }

    const auto* info = reinterpret_cast<const VIDEOINFOHEADER*>(requested.Format());
    for (int i = 0; i < kOfferedCount; ++i) {
        if (static_cast<std::uint32_t>(info->bmiHeader.biWidth) == kOfferedGeometries[i].width &&
            static_cast<std::uint32_t>(info->bmiHeader.biHeight) == kOfferedGeometries[i].height) {
            selected_ = i;
            break;
        }
    }

    // A peer that is already connected has to agree before the connection can be rebuilt around the
    // new geometry; if it will not, the selection above still stands for the next connection.
    if (IsConnected()) {
        if (m_Connected->QueryAccept(media_type) != S_OK) {
            return VFW_E_INVALIDMEDIATYPE;
        }
        IFilterGraph* const graph = m_pFilter->GetFilterGraph();
        if (graph != nullptr) {
            const HRESULT hr = graph->Reconnect(this);
            if (FAILED(hr)) {
                return hr;
            }
        }
    }
    m_mt = requested;
    return S_OK;
}

HRESULT CUnifiedStreamCameraStream::CheckMediaType(const CMediaType* media_type) {
    CheckPointer(media_type, E_POINTER);

    if (*media_type->Type() != MEDIATYPE_Video || *media_type->Subtype() != MEDIASUBTYPE_I420 ||
        *media_type->FormatType() != FORMAT_VideoInfo) {
        return E_INVALIDARG;
    }
    if (media_type->FormatLength() < sizeof(VIDEOINFOHEADER) || media_type->Format() == nullptr) {
        return E_INVALIDARG;
    }

    const auto* info = reinterpret_cast<const VIDEOINFOHEADER*>(media_type->Format());
    if (info->bmiHeader.biCompression != MAKEFOURCC('I', '4', '2', '0')) {
        return E_INVALIDARG;
    }
    if (info->bmiHeader.biWidth <= 0 || info->bmiHeader.biHeight <= 0) {
        return E_INVALIDARG;
    }

    // Only the geometries this pin offers. A filter that accepted anything would be asked for
    // something it cannot allocate for.
    for (const auto& geometry : kOfferedGeometries) {
        if (static_cast<std::uint32_t>(info->bmiHeader.biWidth) == geometry.width &&
            static_cast<std::uint32_t>(info->bmiHeader.biHeight) == geometry.height) {
            return S_OK;
        }
    }
    return E_INVALIDARG;
}

HRESULT CUnifiedStreamCameraStream::DecideBufferSize(IMemAllocator* allocator,
                                                     ALLOCATOR_PROPERTIES* request) {
    CheckPointer(allocator, E_POINTER);
    CheckPointer(request, E_POINTER);
    CAutoLock lock(m_pFilter->pStateLock());

    // Sized for the largest geometry this pin offers rather than for the connected one, so nothing
    // on the streaming path can ever need a buffer larger than the one negotiated here.
    request->cBuffers = 1;
    request->cbBuffer = static_cast<long>(unifiedstream::kMaxPayloadBytes);
    if (request->cbAlign == 0) {
        request->cbAlign = 1;
    }

    ALLOCATOR_PROPERTIES actual{};
    const HRESULT hr = allocator->SetProperties(request, &actual);
    if (FAILED(hr)) {
        return hr;
    }
    if (actual.cbBuffer < request->cbBuffer || actual.cBuffers < request->cBuffers) {
        return E_FAIL;
    }
    return S_OK;
}

HRESULT CUnifiedStreamCameraStream::SetMediaType(const CMediaType* media_type) {
    CAutoLock lock(m_pFilter->pStateLock());

    const HRESULT hr = CSourceStream::SetMediaType(media_type);
    if (FAILED(hr)) {
        return hr;
    }

    const auto* info = reinterpret_cast<const VIDEOINFOHEADER*>(m_mt.Format());
    if (info == nullptr) {
        return E_UNEXPECTED;
    }
    width_ = static_cast<std::uint32_t>(info->bmiHeader.biWidth);
    height_ = static_cast<std::uint32_t>(info->bmiHeader.biHeight);
    frame_bytes_ = I420Bytes(width_, height_);
    frame_length_ = info->AvgTimePerFrame > 0 ? info->AvgTimePerFrame : kFrameLength;
    frame_interval_ms_ = static_cast<std::uint64_t>(frame_length_) / 10000u;
    if (frame_interval_ms_ == 0) {
        frame_interval_ms_ = 1;
    }

    // Connection time is the last moment anything on the streaming path may allocate. Every buffer
    // `FillBuffer` touches is sized here, once, including the placeholder — which is rendered now
    // rather than regenerated per frame, because three `memset`s over a 1080p frame thirty times a
    // second is real work inside somebody else's process for a picture that never changes.
    ring_frame_.assign(unifiedstream::kMaxPayloadBytes, 0);
    held_frame_.assign(frame_bytes_, 0);
    placeholder_.assign(frame_bytes_, 0);
    unifiedstream::FillPlaceholderI420(placeholder_.data(), width_, height_);
    have_held_frame_ = false;

    return S_OK;
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

HRESULT CUnifiedStreamCameraStream::OnThreadCreate() {
    next_due_ms_ = 0;
    last_end_ = 0;
    have_time_base_ = false;
    time_base_rt_ = 0;
    time_base_us_ = 0;
    first_sample_ = true;
    have_held_frame_ = false;
    next_attach_ms_ = 0;

    // One attempt now, so the ordinary case — a stream already running when the application selects
    // the camera — shows video on the very first frame rather than half a second in.
    MaintainAttachment();
    return NOERROR;
}

HRESULT CUnifiedStreamCameraStream::OnThreadDestroy() {
    consumer_.Detach();
    section_.Close();
    // Not sticky across a stop: a filter that refused for the rest of the process because of one
    // bad mapping would keep refusing after the user installed the matching desktop.
    version_disagreement_ = false;
    return NOERROR;
}

void CUnifiedStreamCameraStream::MaintainAttachment() noexcept {
    if (version_disagreement_ || consumer_.attached()) {
        return;
    }
    const std::uint64_t now = TickMs();
    if (now < next_attach_ms_) {
        return;
    }
    next_attach_ms_ = now + kAttachRetryMs;

    if (!section_.mapped() && !section_.Open()) {
        // No producer has ever created the section. Ordinary, and not an error: the placeholder is
        // the right picture and the next tick tries again.
        return;
    }

    std::uint32_t found = 0;
    switch (consumer_.Attach(section_.region(), &found)) {
        case AttachStatus::Ok:
            ring_generation_ = consumer_.generation();
            have_time_base_ = false;
            return;
        case AttachStatus::NotOurs:
            // The producer clears the magic first during initialisation and writes it last, so a
            // filter attaching mid-initialisation sees exactly this. A retry, not a broken camera —
            // and the section stays open, because it is the same section the producer is filling in.
            return;
        case AttachStatus::Version:
            // Terminal for this attachment. Retrying could only fail identically, and the user-
            // visible resolution comes from the desktop, which reads the declared version out of
            // the registry and refuses the stream before any of this runs.
            consumer_.Detach();
            section_.Close();
            version_disagreement_ = true;
            return;
        case AttachStatus::Malformed:
        case AttachStatus::RegionTooSmall:
            // Neither is a version skew and neither is ours to interpret. Drop the mapping and try
            // again on the timer, in case a healthy producer replaces it.
            consumer_.Detach();
            section_.Close();
            return;
    }
}

void CUnifiedStreamCameraStream::PaceFrame() noexcept {
    const std::uint64_t now = TickMs();
    if (next_due_ms_ == 0) {
        next_due_ms_ = now + frame_interval_ms_;
        return;
    }
    if (now < next_due_ms_) {
        std::uint64_t wait = next_due_ms_ - now;
        // Bounded by one frame interval whatever the clock does. This paces the graph; it is not a
        // wait on the producer, and there is no state of the desktop that can make it longer.
        wait = (std::min)(wait, frame_interval_ms_);
        ::Sleep(static_cast<DWORD>(wait));
    }
    const std::uint64_t after = TickMs();
    next_due_ms_ += frame_interval_ms_;
    if (next_due_ms_ + frame_interval_ms_ < after) {
        // Fallen far behind — the host descheduled this thread, or the machine slept. Resynchronise
        // rather than sprinting to catch up with a burst of frames.
        next_due_ms_ = after + frame_interval_ms_;
    }
}

void CUnifiedStreamCameraStream::TimeSample(bool have_new_frame, std::uint64_t timestamp_us,
                                            REFERENCE_TIME* start, REFERENCE_TIME* end) noexcept {
    REFERENCE_TIME candidate = last_end_;

    if (have_new_frame) {
        if (!have_time_base_) {
            // Anchor the producer's clock to ours once, at the first frame of this attachment. The
            // slot's timestamp is microseconds on the desktop's own timeline and means nothing to
            // this graph until it is rebased.
            have_time_base_ = true;
            time_base_rt_ = last_end_;
            time_base_us_ = timestamp_us;
        }
        const std::uint64_t elapsed_us =
            timestamp_us > time_base_us_ ? timestamp_us - time_base_us_ : 0;
        candidate = time_base_rt_ + static_cast<REFERENCE_TIME>(elapsed_us) * 10;
    }

    // Monotonic whatever the producer's clock did: a restart, a resolution change, or a sample that
    // arrived out of order must not hand the graph a timestamp that goes backwards.
    if (candidate < last_end_) {
        candidate = last_end_;
    }
    *start = candidate;
    *end = candidate + frame_length_;
    last_end_ = *end;
}

HRESULT CUnifiedStreamCameraStream::FillBuffer(IMediaSample* sample) {
    CheckPointer(sample, E_POINTER);

    BYTE* buffer = nullptr;
    const HRESULT hr = sample->GetPointer(&buffer);
    if (FAILED(hr)) {
        return hr;
    }
    if (buffer == nullptr || static_cast<std::size_t>(sample->GetSize()) < frame_bytes_) {
        // The allocator disagreed with `DecideBufferSize`. Nothing to deliver into, and nothing
        // this filter can do about it.
        return E_UNEXPECTED;
    }

    PaceFrame();
    MaintainAttachment();

    // Decision 3's table, and the whole of it. Every branch below ends in a frame; none of them
    // waits, and none of them returns without one.
    bool have_new_frame = false;
    std::uint64_t timestamp_us = 0;

    if (consumer_.attached()) {
        const Liveness liveness = consumer_.GetLiveness(TickMs());
        if (liveness == Liveness::Stopped || liveness == Liveness::ProducerGone) {
            // A clean stop is observed immediately; a desktop that died without stopping is
            // concluded gone once the heartbeat has been stale for two seconds. Either way the
            // held frame is no longer the truth, so it is dropped and the placeholder takes over.
            have_held_frame_ = false;
        } else {
            if (consumer_.generation() != ring_generation_) {
                // A resolution change, or a producer that restarted against this very section. The
                // geometry is already re-read inside the consumer; what has to be dropped here is
                // the timestamp anchor, which belonged to the previous stream.
                ring_generation_ = consumer_.generation();
                have_time_base_ = false;
            }

            FrameInfo info{};
            switch (consumer_.Read(ring_frame_.data(), ring_frame_.size(), &info)) {
                case ReadOutcome::Frame:
                    // A frame whose payload does not match the geometry it claims is a frame
                    // straddling a geometry change. Held rather than scaled from bytes that are
                    // partly the previous resolution's.
                    if (info.width > 0 && info.height > 0 && info.width % 2 == 0 &&
                        info.height % 2 == 0 && info.width <= unifiedstream::kMaxWidth &&
                        info.height <= unifiedstream::kMaxHeight &&
                        info.bytes == I420Bytes(info.width, info.height)) {
                        unifiedstream::ScaleOrCopyI420(ring_frame_.data(), info.width, info.height,
                                                       held_frame_.data(), width_, height_);
                        have_held_frame_ = true;
                        have_new_frame = true;
                        timestamp_us = info.timestamp_us;
                    }
                    break;
                case ReadOutcome::NoFrame:
                case ReadOutcome::BeingWritten:
                case ReadOutcome::Torn:
                    // Nothing new, the producer holds the slot, or the copy was overtaken. All
                    // three hold the previous picture and cost exactly this frame.
                    break;
            }
        }
    }

    if (have_held_frame_) {
        std::memcpy(buffer, held_frame_.data(), frame_bytes_);
    } else {
        std::memcpy(buffer, placeholder_.data(), frame_bytes_);
    }

    REFERENCE_TIME start = 0;
    REFERENCE_TIME end = 0;
    TimeSample(have_new_frame, timestamp_us, &start, &end);
    sample->SetTime(&start, &end);
    sample->SetActualDataLength(static_cast<long>(frame_bytes_));
    sample->SetSyncPoint(TRUE);
    sample->SetDiscontinuity(first_sample_ ? TRUE : FALSE);
    first_sample_ = false;

    return S_OK;
}
