@echo off
set "PATH=C:\msys64\ucrt64\bin;%PATH%"
start "" "%~dp0target\release\animecix.exe" %*
