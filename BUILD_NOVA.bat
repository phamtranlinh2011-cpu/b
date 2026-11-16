@echo off
setlocal
cd /d "%~dp0"

echo ======================================================
echo   NOVA Native 0.5.7 - Windows EXE + USB Packager
echo ======================================================
echo.
echo This script tests and builds NOVA on a Windows developer machine.
echo The finished USB package does NOT need Rust or Cargo.
echo.

powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\build-portable.ps1"
if errorlevel 1 (
    echo.
    echo [ERROR] Build failed. Review the message above.
    pause
    exit /b 1
)

echo.
echo [SUCCESS] Portable package is in the release folder.
pause
