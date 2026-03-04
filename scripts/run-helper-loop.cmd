@echo off
setlocal

set ROOT=%~dp0..

call "%~dp0build-rust.cmd"
if errorlevel 1 exit /b %ERRORLEVEL%

pushd "%ROOT%"
cargo run -p helper -- --loop --interval-secs 60 --window-secs 300
set RESULT=%ERRORLEVEL%
popd

exit /b %RESULT%
