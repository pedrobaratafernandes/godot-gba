#!/bin/bash
# Builds everything: Web (Docker), Linux (Docker) and — on Windows — the native DLL.
set -e
cd "$(dirname "$0")"
bash build_web_docker.sh
bash build_linux_docker.sh
if [[ "$OSTYPE" == msys* || "$OSTYPE" == cygwin* ]]; then
    cmd //c build_windows.bat
fi
