# NOVA Native 0.3.1 — Huong dan tieng Viet

## NOVA Native la gi?

NOVA Native la desktop environment portable chay tren Windows. Day chua phai kernel he dieu hanh rieng va chua boot doc lap tu USB.

## Cach lay ban EXE ma khong cai bo cong cu tren may cua ban

Source ZIP hien tai chua co `NOVA.exe` da bien dich. De tao EXE ma khong cai Rust/Cargo tren may cua ban, dung GitHub Actions:

1. Dang nhap GitHub va tao mot repository moi (private cung duoc).
2. Giai nen source ZIP nay tren may.
3. Tai len GitHub **toan bo noi dung** ben trong thu muc `NOVA-OS-Native-v0.3.1`, bao gom thu muc an `.github`.
4. Mo tab **Actions** trong repository va cho phep GitHub Actions neu duoc hoi.
5. Chon workflow **Build NOVA Native for Windows** o cot ben trai.
6. Bam **Run workflow** va chay tren nhanh mac dinh.
7. Khi job thanh cong, mo run vua hoan tat va tai artifact `NOVA-OS-Native-v0.3.1-USB` trong muc Artifacts.
8. Giai nen ZIP artifact va copy toan bo noi dung vao USB.

Workflow se chay unit tests, build `NOVA.exe`, tao thu muc portable va dong goi ZIP. Neu workflow that bai, mo log cua buoc loi va gui log do de sua.

## Cach chay tren may dich

1. Cam USB vao may Windows 10/11 x64.
2. Mo USB trong File Explorer.
3. Double-click `NOVA.exe`.
4. Khong can cai Node.js, npm, Rust, Cargo, Visual Studio Build Tools, WebView2 hay NOVA installer tren may dich.
5. Khi xong, dong NOVA truoc khi safely eject USB.

Windows khong tu dong chay ung dung bat ky chi vi USB duoc cam vao. Vi vay buoc mo `NOVA.exe` bang tay la gioi han bao mat cua Windows, khong phai cai dat them.

## Khong duoc tach cac thanh phan nay

```text
NOVA-Portable/
  NOVA.exe
  portable.flag
  README_PORTABLE.txt
  Data/
    Users/Default/
    Settings/
    Logs/
  Backup/
```

`NOVA.exe` la executable duy nhat. `portable.flag` va `Data/` la tep/thu muc phu can thiet de NOVA nhan biet che do USB va luu du lieu. Khong chi copy rieng EXE.

## Gioi han va canh bao

- CMD chay lenh that voi quyen cua tai khoan Windows hien tai; no khong phai sandbox.
- Wi-Fi can WLAN AutoConfig va driver Wi-Fi cua may. Ban dau ho tro mang mo va WPA2-Personal (AES).
- Windows co the hien SmartScreen/antivirus warning voi EXE chua ky so. Chi cho phep neu ban tin tuong nguon build va da xac minh artifact.
- Khong rut USB trong luc NOVA dang ghi du lieu.
- Chua co benchmark hoac kiem thu tren nhieu may phan cung; can xac nhan tren may dich truoc khi dung thuong xuyen.


## Ghi chu ban 0.3.1

- Files kiem tra duong dan canonical truoc khi mo thu muc, mo tep va tao thu muc moi.
- Neu USB/thu muc Settings chi doc, NOVA hien loi luu Settings tren thanh trang thai.
- Tep XML Wi-Fi tam thoi duoc don dep khi thao tac ket thuc theo luong Rust thong thuong. Tuy nhien tep van co the con lai neu app bi tat dot ngot; khong nen dung tren may/tai khoan Windows khong tin cay.
- CMD va Wi-Fi can kiem thu thuc te tren Windows 10/11 truoc khi su dung hang ngay.
