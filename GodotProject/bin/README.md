# Binaries Directory

Compiled GDExtension libraries. They are produced by the scripts in `../build/`.

### Expected files
*   **godot_gba.dll** — Windows (also used by the editor and F5) — `build/build_windows.bat`
*   **libgodot_gba.so** — Linux — `build/build_linux_docker.sh`
*   **godot_gba.wasm** — Web without threads (the project's Web preset) — `build/build_web_docker.sh`
*   **godot_gba.threads.wasm** — Web with threads — `build/build_web_docker.sh`

To build everything: `bash build/build_all.sh`
