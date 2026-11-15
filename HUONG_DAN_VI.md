# NOVA Native 0.5.1 — Hướng dẫn tiếng Việt

## Security Center và 5 công cụ GUI (0.5.1)

NOVA có workspace cho Zenmap/Nmap, Greenbone GSA, OWASP ZAP, Wireshark và Burp Suite. Các engine bên thứ ba phải được cài riêng từ trang chính thức; Greenbone cần máy chủ/scanner riêng. NOVA không nhúng engine quét vào USB.

Trong Security Center, nhập phạm vi tài sản được phép và xác nhận quyền kiểm thử trước khi mở workspace. Đây là bước nhắc nhở trong quy trình, không phải firewall hoặc cơ chế ràng buộc target bên trong công cụ. Những ứng dụng bên thứ ba được mở trên Windows host, không chạy trong Windows Sandbox. Hãy cấu hình target allowlist tương ứng ngay trong từng công cụ.


## 1. Chạy NOVA từ USB

1. Giải nén gói `NOVA-OS-Native-v0.5.1-source.zip` để build, hoặc giải nén artifact `NOVA-OS-Native-v0.5.1-USB` sau khi GitHub Actions build thành công.
2. Giữ `NOVA.exe`, `portable.flag`, `README_PORTABLE.txt`, thư mục `scripts` và `Data` cùng một thư mục.
3. Cắm USB vào Windows 10/11 x64 và mở `NOVA.exe`.
4. Máy đích không cần Rust, Cargo, Node.js, npm hay WebView2. Máy vẫn cần driver đồ họa tương thích.
5. Đóng NOVA và đóng các cửa sổ Windows Sandbox trước khi rút USB an toàn.

## 2. Gõ tiếng Việt và Unicode

- Mở **UniKey** từ Launcher hoặc thanh Dock.
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

## 6. Build bằng GitHub Actions

Xem `BUILD_WITH_GITHUB.md`. Workflow `.github/workflows/rust.yml` chạy trên `windows-latest`, tạo/refresh `Cargo.lock` khi phiên bản crate gốc thay đổi, chạy `cargo test --locked`, build Release và tạo artifact `NOVA-OS-Native-v0.5.1-USB`.

## 7. Tài liệu

- `README_PORTABLE.txt`: hướng dẫn bản portable trên USB.
- `docs/ARCHITECTURE.md`: kiến trúc.
- `docs/SECURITY_0.5.0.md`: ranh giới bảo mật nền tảng và threat model.
- `docs/RESEARCH_REFERENCES_0.5.0.md`: tài liệu tham khảo cho nền tảng portable, Unicode và Sandbox.
- `RELEASE_NOTES_0.5.1.md`: ghi chú phát hành.
- `docs/SECURITY_GUI_TOOLS_0.5.1.md`: phạm vi và giới hạn bảo mật của 5 công cụ GUI.
- `docs/RESEARCH_REFERENCES_0.5.1.md`: tài liệu gốc cho Security Center và 5 GUI workspace.
