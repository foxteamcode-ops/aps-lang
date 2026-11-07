@echo off
if exist "%~dp0target\release\aps.exe" (
    "%~dp0target\release\aps.exe" %*
) else (
    "%~dp0target\debug\aps.exe" %*
)
