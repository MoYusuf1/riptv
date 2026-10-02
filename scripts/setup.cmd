@echo off
setlocal
rem A cmd child can inherit PowerShell 7 modules that PowerShell 5.1 cannot load.
rem Clear only this process's value so powershell.exe rebuilds its own module paths.
set "PSModulePath="
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0setup.ps1" %*
exit /b %ERRORLEVEL%
