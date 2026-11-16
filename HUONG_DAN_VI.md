# NOVA Native 0.5.7 — Hướng dẫn tiếng Việt

## Security Center và 5 công cụ GUI (0.5.1)

NOVA có workspace cho Zenmap/Nmap, Greenbone GSA, OWASP ZAP, Wireshark và Burp Suite. Các engine bên thứ ba phải được cài riêng từ trang chính thức; Greenbone cần máy chủ/scanner riêng. NOVA không nhúng engine quét vào USB.

Trong Security Center, nhập phạm vi tài sản được phép và xác nhận quyền kiểm thử trước khi mở workspace. Đây là bước nhắc nhở trong quy trình, không phải firewall hoặc cơ chế ràng buộc target bên trong công cụ. Những ứng dụng bên thứ ba được mở trên Windows host, không chạy trong Windows Sandbox. Hãy cấu hình target allowlist tương ứng ngay trong từng công cụ.


## 1. Chạy NOVA từ USB

1. Giải nén gói `NOVA-OS-Native-v0.5.7-Stability-Security.zip` để build, hoặc giải nén artifact `NOVA-OS-Native-v0.5.7-USB` sau khi GitHub Actions build thành công.
2. Giữ `NOVA.exe`, `portable.flag`, `QUICK_START.txt`, thư mục `scripts` và `Data` cùng một thư mục.
3. Cắm USB vào Windows 10/11 x64 và mở `NOVA.exe`.
4. Máy đích không cần Rust, Cargo, Node.js, npm hay WebView2. Máy vẫn cần driver đồ họa tương thích.
5. Đóng NOVA và đóng các cửa sổ Windows Sandbox trước khi rút USB an toàn.

## 2. Gõ tiếng Việt và Unicode

- Mở **Start** ở taskbar phía dưới, chọn **UniKey**.
- Chọn **Download UniKey to USB**. NOVA tải UniKey 4.6 RC2 x64 từ `unikey.org`, kiểm tra chữ ký số và nhà phát hành `PHAM KIM LONG` trước khi cho chạy.
- Chọn **Start UniKey**.
- Mở biểu tượng UniKey ở khay hệ thống Windows, chọn **Bảng mã: Unicode**, **Kiểu gõ: Telex hoặc VNI**.
- NOVA Editor lưu văn bản ở UTF-8; có thể mở UTF-8 và UTF-16 LE/BE có BOM. Windows font fallback được bật để hiển thị thêm glyph nếu máy đã có font phù hợp.
- Terminal yêu cầu CMD dùng code page UTF-8 (`65001`). Một số chương trình console cũ vẫn có thể xuất ra bảng mã OEM/ANSI nên tiếng Việt có thể hiển thị chưa hoàn hảo.
- UniKey chạy trên Windows host, không nằm trong Windows Sandbox. NOVA không cài service hay tác vụ khởi động cùng Windows.

## 3. Bảo vệ khi tải hoặc kiểm tra tệp lạ

**NOVA không phải máy ảo.** Bản thân `NOVA.exe` chạy trên Windows host. Chỉ những thao tác được mở rõ ràng trong **Windows Sandbox** mới được cách ly.

### Kiểm tra một tệp đáng ngờ

1. Đưa tệp vào `Data/Users/Default/Downloads` hoặc thư mục làm việc bên trong NOVA.
2. Mở **Files**, chọn tệp và nhấn **Inspect in Sandbox**.
3. NOVA tạo một bản sao riêng ở `Data/Security/Quarantine`, chỉ chia sẻ bản sao đó ở chế độ read-only vào Windows Sandbox.
4. Sandbox tắt mạng, clipboard, vGPU, audio/video input và printer redirection. Explorer mở thư mục mẫu; bạn phải tự chọn tệp để mở/chạy bên trong Sandbox.
5. Khi xong, đóng Windows Sandbox và xác nhận xóa toàn bộ nội dung phiên.
6. Quay lại **Security Center** → **Clear staged copies** để xóa bản sao mẫu và file `.wsb` do NOVA tạo. Hãy đóng tất cả cửa sổ Sandbox trước khi dọn.

Tệp gốc trong thư mục NOVA không bị sửa hoặc xóa. Bản sao tạm trên USB sẽ vẫn nằm trong `Data/Security/Quarantine` cho đến khi bạn dọn.

### Duyệt web bằng Chrome cách ly

- Mở **Google Chrome** → **Open isolated (recommended)**.
- Chrome chạy trong Windows Sandbox với profile mới; cookie, extension, cache và tệp tải xuống nằm trong máy ảo dùng một lần và bị xóa khi đóng Sandbox.
- Chế độ này bật mạng để trang web hoạt động; mạng nội bộ có thể nhìn thấy từ Sandbox. Clipboard và thư mục host ghi được đều bị tắt.
- Nút **Open on Windows host** phải xác nhận cảnh báo. Chrome bình thường trên Windows không được NOVA cách ly.

## 4. Yêu cầu Windows Sandbox

- Windows Pro, Enterprise hoặc Education; Windows Home không hỗ trợ Windows Sandbox.
- Bật CPU virtualization trong BIOS/UEFI.
- Máy cần Windows build tương thích, ít nhất 4 GB RAM (8 GB khuyến nghị), 1 GB đĩa trống và 2 lõi CPU theo hướng dẫn Microsoft.
- Vào **Turn Windows features on or off**, bật **Windows Sandbox**, khởi động lại nếu được yêu cầu.
- Nếu Sandbox không chạy, NOVA sẽ báo lỗi và không tự chạy tệp lạ trên host để thay thế.

## 5. Những gì vẫn chạy trên Windows host

- NOVA Terminal chạy CMD với quyền của tài khoản Windows hiện tại. Không chạy lệnh không rõ nguồn gốc.
- UniKey là tiến trình Windows riêng, có thể nhận dữ liệu gõ trên toàn hệ thống.
- Wi-Fi sử dụng cấu hình WLAN của Windows.
- Chrome bình thường, nếu bạn chọn Open on Windows host, dùng profile Windows hiện có.
- Files/Editor thông thường ghi vào thư mục dữ liệu NOVA (trong portable mode là USB), không phải một hệ điều hành/VM riêng.

Không có phần mềm nào đảm bảo tuyệt đối trước mọi lỗ hổng Windows, hypervisor, firmware hoặc driver trong tương lai. Giữ Windows và Microsoft Defender luôn cập nhật.

## 6. Build và dependency lockfile

Máy build cần Rust 1.95.0, Microsoft C++ Build Tools/MSVC, Windows SDK và Internet để tải dependency ở lần đầu.

1. Mở PowerShell tại thư mục source.
2. Chạy `powershell -ExecutionPolicy Bypass -File scripts/prepare-lockfile.ps1` để tạo và xác minh `Cargo.lock` nếu đây là lần đầu.
3. Commit `Cargo.lock` vào Git cùng source. CI không tự push hoặc cập nhật lockfile thay bạn.
4. Khi chủ động nâng cấp dependency, chạy `scripts/prepare-lockfile.ps1 -Update`, xem kỹ diff rồi commit.
5. Chạy `BUILD_NOVA.bat` để test, build và đóng gói USB.

GitHub Actions chạy trên `windows-latest` khi push, pull request hoặc chạy thủ công. Pipeline kiểm tra lockfile, Clippy, test Windows và build Release. Repository permission chỉ đọc; nếu thiếu `Cargo.lock`, CI sẽ dừng và chỉ rõ cách tạo file.

### Ký số EXE (tùy chọn)

Chỉ bước ký số riêng sau khi Cargo build/test hoàn tất trên workflow push lên nhánh `main` mới nhận hai GitHub Secrets `NOVA_SIGN_CERT_BASE64` và `NOVA_SIGN_CERT_PASSWORD`. Cargo và build script của dependency không nhận hai secret này. Pull request và chạy thủ công luôn build unsigned để tránh đưa chứng thư cho mã chưa tin cậy. Secret thứ nhất là PFX code-signing được mã hóa Base64; secret thứ hai là mật khẩu PFX. Không đưa PFX hoặc mật khẩu vào source. Nếu thiếu hai secret, EXE không có chữ ký Authenticode nhưng vẫn có checksum SHA-256.

Sau khi ký EXE thủ công, chạy `scripts/finalize-portable.ps1` để cập nhật BUILD_INFO, checksum và ZIP.

## 7. Đo CPU/RAM

Chạy NOVA trên Windows, mở PowerShell và chạy:

`powershell -ExecutionPolicy Bypass -File scripts/measure-performance.ps1 -DurationSeconds 120`

Script ghi một mẫu mỗi giây vào CSV: CPU đã chuẩn hóa theo tổng số logical CPU của máy, working set, private memory, handles và threads. Kết quả nằm trong `perf-results/`. Để đo khi có nhiều tiến trình NOVA, truyền thêm `-ProcessId <PID>`. Nên đo riêng lúc idle, lúc mở System Monitor và lúc mở nhiều workspace.

## 8. Tài liệu

- `README.md`: tổng quan dự án, cấu trúc và trạng thái kiểm thử.
- `RELEASE_NOTES_0.5.7.md`: thay đổi của bản hiện tại.
- `SECURITY.md`: ranh giới bảo mật và cách báo cáo lỗ hổng riêng tư.
- `QUICK_START.txt`: hướng dẫn nhanh được tạo tự động trong gói USB.
