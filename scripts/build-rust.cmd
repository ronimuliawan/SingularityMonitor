@echo off
setlocal

call "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64 >nul
if errorlevel 1 (
    echo Failed to load Visual Studio developer command environment.
    exit /b 1
)

set ROOT=%~dp0..
pushd "%ROOT%"
cargo build --workspace %*
set RESULT=%ERRORLEVEL%
popd

exit /b %RESULT%
