@echo off
rem AnimeciX Windows launcher - GTK4/Adwaita via MSYS2 UCRT64
rem Repo: https://github.com/Lowell137/animecix-linux (windows-port branch)
setlocal
if exist "C:\msys64\ucrt64\bin\libgtk-4-1.dll" set "PATH=C:\msys64\ucrt64\bin;%PATH%"
start "" "%~dp0animecix.exe" %*
