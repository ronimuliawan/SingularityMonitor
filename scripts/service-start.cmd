@echo off
setlocal
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0service-daemon.ps1" -Action start %*
