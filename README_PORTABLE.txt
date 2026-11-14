NOVA OS NATIVE 0.3.2 — USB PORTABLE EDITION

BAT DAU
1. Giu NOVA.exe, portable.flag va thu muc Data/ cung nam trong mot thu muc.
2. Cam USB vao may Windows 10/11 x64, mo USB trong File Explorer, double-click NOVA.exe.
3. Khong can cai Node.js, npm, Rust, Cargo, WebView2, bo cai NOVA hay ket noi Internet de CHAY goi nay.
4. NOVA luu tep nguoi dung va tuy chon trong Data/ canh NOVA.exe.
5. Dong NOVA truoc khi thao USB an toan.

UNG DUNG
- Files: quan ly tep trong Data/Users/Default.
- Terminal: chay lenh Windows CMD bang quyen tai khoan hien tai.
- Editor, Calculator, Settings, System Monitor.

LENH WI-FI
  wifi
  wifi -h
  wifi connect "Home WiFi" "your-password"
  wifi select "Home WiFi"
  password "your-password"
  password delete
  exit

BAO MAT
- CMD khong phai sandbox. Lenh chay bang quyen tai khoan Windows va co the thay doi may that.
- NOVA khong luu mat khau Wi-Fi trong Data/settings/log cua minh. Windows WLAN quan ly profile da luu.
- Tep XML WLAN chua mat khau duoc tao tam thoi trong TEMP cua tai khoan Windows de netsh nhap profile; NOVA se thu xoa tep khi ket thuc thao tac. Tep co the con lai neu tien trinh bi ket thuc dot ngot; ban nay chua dat ACL rieng cho tep. Windows quan ly profile da nhap.
- NOVA chi xoa profile Wi-Fi ma chinh NOVA da ghi nhan.

GIOI HAN
- Day la desktop environment portable chay tren Windows, khong phai kernel OS rieng hay Live USB boot doc lap.
- Wi-Fi can WLAN AutoConfig service va driver Wi-Fi hoat dong.
- Ho tro Wi-Fi mo va WPA2-Personal (AES), mat khau ASCII in duoc hoac khoa hex 64 ky tu; chua co Enterprise/802.1X va captive portal.
- Yeu cau Windows 10/11 x64 va driver do hoa tuong thich.
- Windows co the hien SmartScreen/antivirus warning voi EXE chua ky so. Hay xac minh nguon truoc khi cho phep chay.
