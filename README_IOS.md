# 📱 FLAC Hi-Res Player for iOS (iPhone / iPad)

Ứng dụng phát nhạc Lossless & Hi-Res dành riêng cho iOS, được viết bằng **Swift 5 & SwiftUI**, kết nối trực tiếp đến máy tính PC của bạn qua mạng Wi-Fi nội bộ để stream toàn bộ kho nhạc FLAC.

---

## 🔍 Cơ Chế Hoạt Động: "Link từ IP Localhost qua iPhone"

### ⚠️ Lưu ý quan trọng về khái niệm "localhost":
* Khi bạn gõ `localhost` (hoặc `127.0.0.1`) **trên máy tính**, nó trỏ vào máy tính.
* Nhưng khi bạn gõ `localhost` **trên iPhone**, iOS sẽ hiểu là đang trỏ vào **chính chiếc iPhone**, do đó sẽ **không** tìm thấy nhạc trên PC.
* **Giải pháp chuẩn:** Cả iPhone và PC cùng kết nối chung **1 mạng Wi-Fi**. Máy tính sẽ có một địa chỉ IP nội bộ (LAN IP), ví dụ: `192.168.1.10`.
* App PC đã được tích hợp sẵn máy chủ Streaming siêu nhẹ chạy ở cổng `8080`.
* Trên iPhone, bạn chỉ cần nhập:
  ```
  http://192.168.1.10:8080
  ```
  *(Bạn có thể xem chính xác IP của máy tính mình bằng cách bấm nút màu xanh **"📱 iPhone Link"** trên thanh tiêu đề của app PC).*

---

## ✨ Tính Năng Của App iOS

1. **Phát nhạc chạy nền (Background Audio):** Tiếp tục phát khi khóa màn hình hoặc chuyển sang ứng dụng khác.
2. **Lock Screen Controls & Dynamic Island:** Điều khiển Play/Pause, Next/Prev, thanh tua (scrubber) và hiển thị ảnh bìa Album ngay trên màn hình khóa.
3. **Hi-Res Audio Badges:** Hiển thị nhãn vàng cho các file chất lượng cao `24-bit / 96kHz`, `24-bit / 192kHz`.
4. **Giao diện chuẩn Apple Music:** Album art lớn, hiệu ứng làm mờ hiện đại, thanh tua thời gian chính xác, Shuffle & Repeat.
5. **Mini Player tiện lợi:** Luôn nằm ở đáy màn hình khi duyệt danh sách bài hát.

---

## 🛠 Cách Build & Cài Đặt Lên iPhone

### Cách 1: Tự động Build ra file `.ipa` bằng GitHub Actions (Không cần máy Mac)
Nếu bạn không có máy Mac, bạn có thể tận dụng dịch vụ miễn phí của GitHub:
1. Tạo một repository mới trên GitHub (chế độ Public hoặc Private đều được).
2. Tải toàn bộ thư mục `flac_player_ios` này lên repository đó.
3. Vào tab **Actions** trên GitHub, workflow `Build iOS IPA` sẽ tự động chạy trên máy chủ macOS của GitHub.
4. Sau khoảng 2-3 phút, vào mục **Artifacts** tải file `FlacPlayeriOS.ipa` về máy tính.
5. Cài file `.ipa` này vào iPhone:
   - Dùng **Sideloadly** (miễn phí trên Windows): Cắm iPhone vào PC qua cáp USB, kéo thả file `.ipa` vào Sideloadly, nhập Apple ID và bấm Start để cài đặt.
   - Hoặc dùng **AltStore** / **TrollStore** / **Scarlet**.

### Cách 2: Mở trực tiếp bằng Xcode (Nếu bạn có máy Mac)
1. Cài đặt `xcodegen` trên Mac: `brew install xcodegen`
2. Mở Terminal tại thư mục `flac_player_ios` và gõ:
   ```bash
   xcodegen generate
   open FlacPlayeriOS.xcodeproj
   ```
3. Cắm iPhone vào máy Mac, chọn thiết bị của bạn và nhấn **Run (Cmd + R)** để cài lên máy.

---

## 🎁 Mẹo Bổ Sung: Nghe Ngay Bằng Các App iOS Nổi Tiếng
Máy chủ streaming trên PC được thiết kế theo chuẩn HTTP Media Streaming. Vì vậy, ngay cả khi chưa kịp cài app SwiftUI, bạn cũng có thể:
1. Mở app **VLC** hoặc **FLACbox** hoặc **Foobar2000** trên App Store của iPhone.
2. Chọn mục **Network / Stream URL**.
3. Dán đường link:
   - Stream bài hát: `http://192.168.1.10:8080/api/stream/0` (thay số 0 bằng vị trí bài)
   - Lấy danh sách JSON: `http://192.168.1.10:8080/api/tracks`
