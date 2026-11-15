@echo off
REM Web (WASM) build of Godot GBA on Windows, through Docker Desktop.
REM Does the same as build_web_docker.sh (requires Git for Windows).
setlocal
where bash >nul 2>nul
if errorlevel 1 (
    if exist "%ProgramFiles%\Git\bin\bash.exe" (
        set "BASH=%ProgramFiles%\Git\bin\bash.exe"
    ) else (
        echo Error: bash not found. Install Git for Windows or run build_web_docker.sh from Git Bash.
        exit /b 1
    )
) else (
    set "BASH=bash"
)
"%BASH%" "%~dp0build_web_docker.sh"
