@echo off
setlocal

set "VSDEVCMD="

if defined ProgramFiles(x86) (
    set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
    if exist "%VSWHERE%" (
        for /f "usebackq delims=" %%I in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do (
            set "VSDEVCMD=%%I\Common7\Tools\VsDevCmd.bat"
        )
    )
)

if not defined VSDEVCMD (
    for %%P in (
        "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\Common7\Tools\VsDevCmd.bat"
        "C:\Program Files (x86)\Microsoft Visual Studio\17\BuildTools\Common7\Tools\VsDevCmd.bat"
        "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat"
        "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat"
        "C:\Program Files\Microsoft Visual Studio\2022\Professional\Common7\Tools\VsDevCmd.bat"
        "C:\Program Files\Microsoft Visual Studio\2022\Enterprise\Common7\Tools\VsDevCmd.bat"
    ) do (
        if not defined VSDEVCMD if exist %%~P set "VSDEVCMD=%%~P"
    )
)

if defined VSDEVCMD (
    call "%VSDEVCMD%" -arch=x64 -host_arch=x64 >nul
    if errorlevel 1 (
        echo Failed to load Visual Studio developer command environment from "%VSDEVCMD%".
        exit /b 1
    )
) else (
    echo VsDevCmd.bat not found. Continuing with current environment.
)

set ROOT=%~dp0..
pushd "%ROOT%"
cargo build --workspace %*
set RESULT=%ERRORLEVEL%
popd

exit /b %RESULT%
