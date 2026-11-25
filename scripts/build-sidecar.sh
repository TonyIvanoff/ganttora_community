#!/usr/bin/env bash
# scripts/build-sidecar.sh: builds the MPXJ sidecar and puts it where Tauri
# looks for it.
#
# WHAT THIS SCRIPT IS FOR
# Ganttora reads MS Project .mpp files with a helper program, the "MPXJ
# sidecar" (backend/sidecar/mpxj/). This script builds that helper
# and copies it to backend/src-tauri/binaries/ under the name Tauri expects.
#
# WHY THE STRANGE FILE NAME
# Tauri ships one sidecar per platform and finds the right one by a suffix:
# the Rust "target triple" (a name for operating system + CPU type), e.g.
#
#     backend/src-tauri/binaries/mpxj-sidecar-aarch64-apple-darwin        (Mac, Apple Silicon)
#     backend/src-tauri/binaries/mpxj-sidecar-x86_64-apple-darwin         (Mac, Intel)
#     backend/src-tauri/binaries/mpxj-sidecar-x86_64-pc-windows-msvc.exe  (Windows)
#     backend/src-tauri/binaries/mpxj-sidecar-x86_64-unknown-linux-gnu    (Linux)
#
# We ask the Rust compiler for the triple of this machine, so the name is
# always correct.
#
# TWO MODES
#   scripts/build-sidecar.sh           native mode (default, used by CI):
#                                      needs GraalVM native-image on PATH.
#                                      Produces a real program; users need no Java.
#   scripts/build-sidecar.sh --jar     developer mode (macOS/Linux only):
#                                      needs only Java 17+. Builds the fat jar
#                                      and installs a tiny launcher script that
#                                      runs `java -jar`. NEVER ship this.
#
# Beginner note: this is a "bash" script, a list of terminal commands saved in
# a file. Run it from any folder: `bash scripts/build-sidecar.sh`.

# "Strict mode": stop at the first failing command (-e), treat unknown
# variables as errors (-u), and fail a pipeline if any part fails (pipefail).
# Without this, a script can keep going after an error and produce a broken result.
set -euo pipefail

# --- Where things are ------------------------------------------------------
# The folder this script is in, then the repository root one level up.
# This makes the script work no matter which folder you run it from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SIDECAR_DIR="$REPO_ROOT/backend/sidecar/mpxj"
BIN_DIR="$REPO_ROOT/backend/src-tauri/binaries"

MODE="native"
if [[ "${1:-}" == "--jar" ]]; then
  MODE="jar"
elif [[ $# -gt 0 ]]; then
  echo "usage: $0 [--jar]" >&2
  exit 1
fi

# --- Which platform are we on? ---------------------------------------------
# `rustc -vV` prints details about the Rust compiler, including a line like
# "host: aarch64-apple-darwin". We keep only the part after "host: ".
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
if [[ -z "$TRIPLE" ]]; then
  echo "build-sidecar: could not find the Rust target triple (is rustc installed?)" >&2
  exit 1
fi

# Windows programs end in .exe; others have no extension.
EXT=""
if [[ "$TRIPLE" == *windows* ]]; then
  EXT=".exe"
fi

# The Gradle wrapper: gradlew on macOS/Linux, gradlew.bat on Windows.
GRADLEW="./gradlew"
if [[ "$TRIPLE" == *windows* ]]; then
  GRADLEW="./gradlew.bat"
fi

mkdir -p "$BIN_DIR"
TARGET="$BIN_DIR/mpxj-sidecar-$TRIPLE$EXT"

cd "$SIDECAR_DIR"

if [[ "$MODE" == "native" ]]; then
  # --- Native mode: compile with GraalVM native-image ----------------------
  if ! command -v native-image >/dev/null 2>&1; then
    echo "build-sidecar: native-image not found." >&2
    echo "  Install GraalVM (Java 21+) and put its bin/ folder on PATH," >&2
    echo "  or run '$0 --jar' for a developer-only build that needs just Java." >&2
    exit 1
  fi
  echo "build-sidecar: building native sidecar for $TRIPLE (this takes a few minutes)…"
  "$GRADLEW" --console=plain nativeCompile
  cp "build/native/nativeCompile/mpxj-sidecar$EXT" "$TARGET"
else
  # --- Developer mode: fat jar + launcher script ---------------------------
  if [[ "$TRIPLE" == *windows* ]]; then
    echo "build-sidecar: --jar mode is for macOS/Linux only." >&2
    exit 1
  fi
  echo "build-sidecar: building fat jar (developer mode)…"
  "$GRADLEW" --console=plain shadowJar
  JAR="$SIDECAR_DIR/build/libs/mpxj-sidecar-all.jar"
  # Write a two-line launcher. "exec" replaces the launcher with Java, so the
  # exit code and stdout of the sidecar reach Tauri unchanged; "$@" passes on
  # all arguments.
  cat > "$TARGET" <<EOF
#!/bin/sh
# DEVELOPER-ONLY launcher made by scripts/build-sidecar.sh --jar. Do not ship.
exec java -jar "$JAR" "\$@"
EOF
fi

# Make the file executable (allowed to run as a program) on macOS/Linux.
chmod +x "$TARGET"
echo "build-sidecar: installed $TARGET"
