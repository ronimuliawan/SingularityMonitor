@echo off
setlocal

call "%~dp0build-rust.cmd" --release
if errorlevel 1 exit /b %ERRORLEVEL%

call "%~dp0build-viewer.cmd"
if errorlevel 1 exit /b %ERRORLEVEL%

echo Build completed.
