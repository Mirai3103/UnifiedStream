# UnifiedStream Roadmap

## Hiện trạng

Linux MVP đã có đủ ba luồng chính: camera điện thoại thành webcam, microphone điện thoại cho PC và âm thanh PC phát trên điện thoại. Nền tảng kết nối LAN, pairing, reconnect và telemetry đã hoạt động. Rust, Android và frontend hiện build/test thành công.

Roadmap này ưu tiên hoàn thiện Linux trước. Mỗi tính năng lớn phải có một OpenSpec change riêng theo thứ tự: `proposal → specs/design → tasks → implementation → verify → archive`.

## 1. Dọn OpenSpec và Git

- [ ] Quyết định và commit migration `.claude` sang `.agents` hoặc khôi phục file cũ.
- [ ] Đảm bảo `openspec validate --all` pass.

**Tại sao cần:** trạng thái kế hoạch phải sạch và đáng tin trước khi tiếp tục phát triển.

## 2. Cải thiện FPS camera

- [ ] Tạo OpenSpec change mới khi bắt đầu giai đoạn này; không dùng lại change rỗng cũ.
- [ ] Đo capture FPS, thời gian chuyển YUV/NV21, JPEG encode, kích thước frame, queue drop, packet loss và FPS nhận được.
- [ ] Đo thực tế ở 480p, 720p và 1080p.
- [ ] Tối ưu theo bottleneck đã đo: ưu tiên buffer reuse, JPEG quality, sau đó mới cân nhắc encode thread.

**Tại sao cần:** camera hiện chạy được nhưng khoảng 16 FPS ở 480p; đo trước giúp tránh tối ưu sai chỗ. H.264 chưa gộp vào giai đoạn này.

## 3. Ổn định camera end-to-end

- [ ] Kiểm tra permission bị từ chối và máy thiếu `v4l2loopback`.
- [ ] Kiểm tra reconnect khi đang stream, đổi camera và đổi độ phân giải.
- [ ] Xác nhận thoát app giải phóng virtual camera hoàn toàn.
- [ ] Chạy lâu để kiểm tra nhiệt độ, RAM và latency tích tụ.

**Tại sao cần:** các đường chạy chính đã hoạt động nhưng một số tình huống lỗi mới chỉ được kiểm tra qua code path, chưa xác minh đầy đủ trên thiết bị thật.

## 4. Quality gate và CI

- [ ] Sửa các cảnh báo Clippy hiện tại.
- [ ] Thêm CI chạy Rust test/Clippy, Android unit test, frontend build và OpenSpec validation.
- [ ] Không cho merge khi một quality gate thất bại.

**Tại sao cần:** ngăn tính năng mới làm hỏng phần camera, audio hoặc protocol đã ổn định.

## 5. Phát hành Linux MVP

- [ ] Viết README cài đặt và sử dụng thực tế.
- [ ] Hướng dẫn `v4l2loopback`, PipeWire, firewall, mDNS và permissions.
- [ ] Đóng gói desktop app, APK và tạo bản release thử nghiệm.
- [ ] Viết troubleshooting ngắn cho các lỗi phổ biến.

**Tại sao cần:** biến bản dev đang chạy được thành sản phẩm người khác có thể tự cài và sử dụng.

## 6. Hoàn thiện trải nghiệm

- [ ] Adaptive quality/bitrate và jitter buffer.
- [ ] Audio resampling, echo cancellation nếu cần.
- [ ] UI/device tests, logging, diagnostics và crash recovery.

**Tại sao cần:** tăng độ ổn định khi mạng yếu, thiết bị khác nhau hoặc chạy trong thời gian dài.

## 7. Windows

- [ ] WASAPI system-audio capture.
- [ ] Virtual microphone backend.
- [ ] Media Foundation/DirectShow virtual camera.
- [ ] Installer, signing và kiểm thử Windows.

**Tại sao cần:** Windows cần backend driver và quy trình đóng gói riêng; chỉ nên bắt đầu sau khi protocol và Linux MVP đã ổn định.

## 8. Opus cho audio — để sau

- [ ] Khi thực sự ưu tiên, tạo OpenSpec change mới cho microphone và speaker Opus.
- [ ] Thêm runtime codec detection, negotiation và fallback về PCM.
- [ ] Kiểm tra chất lượng, latency và packet loss ở cả hai chiều.

**Tại sao cần:** Opus giảm băng thông audio, nhưng PCM hiện đã đủ cho MVP nên chưa cần triển khai sớm.

## 9. H.264 cho camera — để sau

- [ ] Khi thực sự ưu tiên, tạo OpenSpec change mới sau khi đã có số đo MJPEG.
- [ ] Thiết kế codec negotiation, keyframe, packet-loss recovery và desktop decoder.
- [ ] Luôn giữ MJPEG làm fallback.

**Tại sao cần:** H.264 giảm băng thông và hỗ trợ FPS cao hơn, nhưng phức tạp; MJPEG hiện vẫn là baseline an toàn.

## Thứ tự tổng quát

`OpenSpec/Git sạch → FPS profiling → Camera ổn định → CI → Linux release → Polish → Windows → Opus → H.264`
