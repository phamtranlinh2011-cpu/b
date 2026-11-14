@echo off
setlocal
cd /d "%~dp0"

echo ======================================================
echo   NOVA OS 0.3.2 - Native Windows EXE + USB Packager
echo ======================================================
echo.
echo This script is for BUILDING NOVA on a developer machine.
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
