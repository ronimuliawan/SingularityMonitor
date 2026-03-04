@echo off
setlocal

set ROOT=%~dp0..

call "%~dp0build-rust.cmd"
if errorlevel 1 exit /b %ERRORLEVEL%

pushd "%ROOT%"
cargo run -p helper -- --import-history --days 60 --chunk-hours 6
set RESULT=%ERRORLEVEL%
popd

exit /b %RESULT%
