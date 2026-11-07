@echo off
if exist "%~dp0target\release\apm.exe" (
    "%~dp0target\release\apm.exe" %*
) else (
    "%~dp0target\debug\apm.exe" %*
)
