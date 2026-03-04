@echo off
setlocal

set ROOT=%~dp0..
set SM_DATA_ROOT=%ROOT%runtime-data

call "%~dp0build-rust.cmd"
if errorlevel 1 exit /b %ERRORLEVEL%

pushd "%ROOT%"
cargo run -p daemon -- --console
set RESULT=%ERRORLEVEL%
popd

exit /b %RESULT%
