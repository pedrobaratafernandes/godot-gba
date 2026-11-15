@echo off
REM Windows (x86_64) GDExtension build: GodotProject\bin\godot_gba.dll
REM Used by the Godot editor, by F5 and by the Windows export.
REM Close the Godot editor first (otherwise the DLL is locked).
setlocal
cd /d "%~dp0.."
echo Building for Windows (Release)...
cargo build --release || exit /b 1
if not exist "GodotProject\bin" mkdir "GodotProject\bin"
copy /Y "target\release\godot_gba.dll" "GodotProject\bin\godot_gba.dll" || exit /b 1
echo ------------------------------------------------
echo Windows build finished: GodotProject\bin\godot_gba.dll
echo ------------------------------------------------
