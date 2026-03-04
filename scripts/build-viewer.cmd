@echo off
setlocal

set ROOT=%~dp0..
pushd "%ROOT%"
dotnet build "viewer\SingularityMonitor.Viewer.csproj" -c Release %*
set RESULT=%ERRORLEVEL%
popd

exit /b %RESULT%
