# NOVA Native 0.5.7 — Hướng dẫn tiếng Việt

## Security Center và 5 công cụ GUI (0.5.1)

NOVA có workspace cho Zenmap/Nmap, Greenbone GSA, OWASP ZAP, Wireshark và Burp Suite. Các engine bên thứ ba phải được cài riêng từ trang chính thức; Greenbone cần máy chủ/scanner riêng. NOVA không nhúng engine quét vào USB.

Trong Security Center, nhập phạm vi tài sản được phép và xác nhận quyền kiểm thử trước khi mở workspace. Đây là bước nhắc nhở trong quy trình, không phải firewall hoặc cơ chế ràng buộc target bên trong công cụ. Những ứng dụng bên thứ ba được mở trên Windows host, không chạy trong Windows Sandbox. Hãy cấu hình target allowlist tương ứng ngay trong từng công cụ.


## Giao diện desktop và thanh tác vụ kiểu Windows 11

- Thanh tác vụ NOVA là thanh nổi bo góc: cụm Start, Search và ứng dụng nằm giữa; mọi ứng dụng được ghim từ Start xuất hiện thường trực; khi có nhiều icon, dải ứng dụng cuộn ngang và các workspace chưa ghim sẽ xuất hiện khi đang mở. Phía phải có Network, System Monitor (khi đủ chiều rộng), đồng hồ/ngày theo giờ máy Windows và Quick Settings.
- Nhấn chuột phải biểu tượng ứng dụng trên thanh tác vụ để Snap trái, Snap phải hoặc Fill desktop. Vùng Snap chừa khoảng trống cho thanh tác vụ để cửa sổ không che dock; sau animation, cửa sổ vẫn có thể kéo và thay đổi kích thước bình thường.
- Taskbar, Start menu và Quick Settings được vẽ ở lớp nổi phía trước cửa sổ ứng dụng; Start menu và Quick Settings trượt/fade theo easing; icon desktop và taskbar có hiệu ứng hover. Hai vùng đệm trái/phải của taskbar co giãn đối xứng để cụm Start và app vẫn nằm giữa; các icon được cuộn ngang khi không đủ chỗ. Quick Settings và Start Menu tự tính chiều rộng theo cửa sổ, tránh nút bị cắt hoặc tràn khỏi màn hình.
- Các cửa sổ Snap không còn bị ép trở lại chiều rộng tối thiểu lớn sau animation. Settings chuyển sang một cột khi hẹp; Editor, Terminal, Trash, Monitor và các hàng thao tác Security tự xuống dòng khi cần. Có thể tắt hiệu ứng trong Settings → Appearance/System bằng **Reduce animations and visual effects** hoặc trong Quick Settings.
- Quick Settings cho phép đổi Dark/Light, bật/tắt Reduce Motion và mở nhanh Network, Settings, Monitor, Files, Terminal.
- Start Menu có vùng cuộn dọc riêng cho cả Pinned và All apps để các ứng dụng ghim không bị cắt ở cửa sổ thấp; Quick Access tự chuyển 1/2/3 cột theo chiều rộng thực tế.
- File Manager ở chế độ List giữ cột tên và dung lượng trong vùng hiển thị; tên file dài chỉ được rút gọn ở phần chữ hiển thị, tên thật trên ổ đĩa không thay đổi. Các nút xác nhận và thao tác trong cửa sổ hẹp có thể tự xuống dòng.
- Đây là thanh tác vụ nằm bên trong cửa sổ NOVA, không thay thế taskbar Windows và không tự điều khiển âm lượng/pin của Windows host.

## 1. Chạy NOVA từ USB

1. Giải nén gói `NOVA-OS-Native-v0.5.7-Stability-Security.zip` để build, hoặc giải nén artifact `NOVA-OS-Native-v0.5.7-USB` sau khi GitHub Actions build thành công.
2. Giữ `NOVA.exe`, `portable.flag`, `QUICK_START.txt`, `BUILD_INFO.txt`, `THIRD_PARTY_NOTICES.txt`, `Apps/UniKey` và `Data` cùng một thư mục.
3. Cắm USB vào Windows 10/11 x64 và mở `NOVA.exe`.
4. Máy đích không cần Rust, Cargo, Node.js, npm hay WebView2. Máy vẫn cần driver đồ họa tương thích.
5. Đóng NOVA và đóng các cửa sổ Windows Sandbox trước khi rút USB an toàn.

## 2. Gõ tiếng Việt và Unicode

- USB release đã kèm sẵn UniKey 4.6 RC2 x64 tại `Apps/UniKey/UniKeyNT.exe`.
- Khi đóng gói, script tải UniKey từ website chính thức `unikey.org`, kiểm tra cấu trúc ZIP và xác minh chữ ký Authenticode của `PHAM KIM LONG` trước khi đưa vào USB.
- Chọn shortcut **UniKey** trong Start/Desktop. NOVA tự khởi chạy UniKey; không cần tải hoặc cài đặt riêng.
- Mở biểu tượng UniKey trong khay hệ thống Windows, chọn **Bảng mã: Unicode**, **Kiểu gõ: Telex hoặc VNI**.
- NOVA Editor lưu văn bản UTF-8; có thể mở UTF-8 và UTF-16 LE/BE có BOM. Windows font fallback được bật để hiển thị thêm glyph nếu máy đã có font phù hợp.
- Terminal yêu cầu CMD dùng code page UTF-8 (`65001`). Một số chương trình console cũ vẫn có thể xuất ra bảng mã OEM/ANSI nên tiếng Việt có thể hiển thị chưa hoàn hảo.
- UniKey là tiến trình Windows host, có thể nhận dữ liệu gõ trên toàn hệ thống. NOVA dừng tiến trình do NOVA khởi chạy khi thoát; không cài service hay tác vụ tự khởi động Windows.

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

### Google Chrome

- Chọn **Google Chrome** trong Start hoặc trên Desktop.
- NOVA mở ứng dụng Google Chrome đã cài trên Windows bằng cửa sổ Chrome gốc, dùng profile, bookmark, extension và thiết lập hiện có của người dùng.
- Nếu Chrome chưa có, NOVA mở trang tải chính thức `google.com/chrome`.
- Chrome chạy trực tiếp trên Windows host, không được NOVA cách ly bằng Windows Sandbox.

### Ký số EXE (tùy chọn)

Chỉ bước ký số riêng sau khi Cargo build/test hoàn tất trên workflow push lên nhánh `main` mới nhận hai GitHub Secrets `NOVA_SIGN_CERT_BASE64` và `NOVA_SIGN_CERT_PASSWORD`. Cargo và build script của dependency không nhận hai secret này. Pull request và chạy thủ công luôn build unsigned để tránh đưa chứng thư cho mã chưa tin cậy. Secret thứ nhất là PFX code-signing được mã hóa Base64; secret thứ hai là mật khẩu PFX. Không đưa PFX hoặc mật khẩu vào source. Nếu thiếu hai secret, EXE không có chữ ký Authenticode nhưng vẫn có checksum SHA-256.

Sau khi ký EXE thủ công, chạy `scripts/finalize-portable.ps1` để cập nhật BUILD_INFO, checksum và ZIP.

### Máy tính khoa học

- Chọn **Calculator** để mở máy tính khoa học kiểu ClassWiz.
- Chọn đơn vị góc DEG/RAD/GRA; nhập biểu thức như `sin(30)+sqrt(16)`, `nCr(8,2)`, `log(100)` hoặc `2π`.
- Các workspace gồm Calculate, Statistics (1 và 2 biến), Equation, Inequality, Distribution, Calculus, Table, Base-N, Matrix, Vector và Ratio.
- Có đạo hàm và tích phân số bằng sai phân trung tâm và Simpson; các hàm phân phối gồm Normal, Binomial và Poisson. Đây là bộ tính toán khoa học mở rộng theo phong cách ClassWiz, chưa phải bản sao đầy đủ 540+ chức năng của fx-991CW.
- Kết quả gần nhất được lưu trong `Ans`; lịch sử phép tính hiển thị trong Calculate.

## 7. Đo CPU/RAM

Chạy NOVA trên Windows, mở PowerShell và chạy:

`powershell -ExecutionPolicy Bypass -File scripts/measure-performance.ps1 -DurationSeconds 120`

Script ghi một mẫu mỗi giây vào CSV: CPU đã chuẩn hóa theo tổng số logical CPU của máy, working set, private memory, handles và threads. Kết quả nằm trong `perf-results/`. Để đo khi có nhiều tiến trình NOVA, truyền thêm `-ProcessId <PID>`. Nên đo riêng lúc idle, lúc mở System Monitor và lúc mở nhiều workspace.

## 8. Tài liệu

- `README.md`: tổng quan dự án, cấu trúc và trạng thái kiểm thử.
- `RELEASE_NOTES_0.5.7.md`: thay đổi của bản hiện tại.
- `SECURITY.md`: ranh giới bảo mật và cách báo cáo lỗ hổng riêng tư.
- `QUICK_START.txt`: hướng dẫn nhanh được tạo tự động trong gói USB.
