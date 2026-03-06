@echo off
setlocal

set ROOT=%~dp0..
set MODE=build
if /I "%~1"=="--msix" (
    set MODE=msix
    shift
)

pushd "%ROOT%"
if /I "%MODE%"=="msix" (
    powershell -ExecutionPolicy Bypass -File "scripts\release-msix.ps1" -BundleHelperRelease %*
) else (
    dotnet build "viewer\SingularityMonitor.Viewer.csproj" -c Release -p:Platform=x64 %*
)
set RESULT=%ERRORLEVEL%
popd

exit /b %RESULT%
