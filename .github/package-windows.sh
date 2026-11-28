#!/usr/bin/env bash
# Packages the native Windows build into the release zip and proves it starts.
#
#   .github/package-windows.sh [--gtk DIR] [--exe FILE] [--out DIR]
#
# The payload is the GTK 4 runtime the binary links against (its DLLs, the
# gdk-pixbuf loaders, the compiled GLib schemas and the Adwaita icon theme),
# the release executable, and the ANGLE libraries Servo's renderer loads at
# runtime. `share/icons` is not optional: the interface asks the theme for
# Adwaita-only names (a bookmark with no favicon uses `web-browser-symbolic`),
# and with no theme installed GTK draws a missing-image icon instead.
#
# The staged executable is launched before the zip is accepted, so a missing
# runtime DLL fails here instead of in the user's hands. Run it from Git Bash
# on Windows, after `cargo build --release --locked`.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
gtk=${GTK_ROOT:-}
exe=${AXYSBROWSER_EXE:-}
out=$root/target/dist

while [ $# -gt 0 ]; do
    case $1 in
        --gtk) gtk=$2; shift 2 ;;
        --exe) exe=$2; shift 2 ;;
        --out) out=$2; shift 2 ;;
        -h | --help) sed -n '2,20p' "$0"; exit 0 ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

case $(uname -s) in
    MINGW* | MSYS*) ;;
    *)
        echo "package-windows.sh runs on Windows (Git Bash), not $(uname -s)" >&2
        exit 1
        ;;
esac

# The host target directory may hold several triplets; the Windows release one
# is the only one this script can package.
target=${CARGO_TARGET_DIR:-$root/target}/x86_64-pc-windows-msvc/release
[ -n "$exe" ] || exe=$target/axysbrowser.exe
[ -n "$gtk" ] || gtk=/c/gtk

version=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$root/Cargo.toml" | head -1)

if [ -z "$version" ]; then
    echo "cannot read the version from $root/Cargo.toml" >&2
    exit 1
fi

if [ ! -f "$exe" ]; then
    echo "no release binary at $exe; run cargo build --release --locked first" >&2
    exit 1
fi

if [ ! -d "$gtk/bin" ] || [ ! -d "$gtk/share/icons" ]; then
    echo "no GTK 4 runtime at $gtk (pass --gtk DIR or set GTK_ROOT)" >&2
    exit 1
fi

# Servo's surfman renderer loads ANGLE itself; these two are build output, not
# part of the GTK runtime, and their absence panics on the first page. Cargo
# owns the package hash in `build/mozangle-<hash>/out`, and that tree does not
# lay itself out identically everywhere (a CI runner's release profile is not a
# developer's incremental one), so look for the pair instead of one path.
angle=
dll_dirs=$(mktemp --suffix=.txt)

trap 'rm -f "$dll_dirs"' EXIT

if [ -d "$target/build" ]; then
    find "$target/build" -type f -name libEGL.dll -exec dirname {} \; >"$dll_dirs" 2>/dev/null || true
fi

while IFS= read -r dir; do
    if [ -f "$dir/libGLESv2.dll" ]; then
        angle=$dir
        break
    fi
done <"$dll_dirs"

if [ -z "$angle" ]; then
    echo "no ANGLE output (libEGL.dll and libGLESv2.dll) under $target/build; build for the Windows target first" >&2
    # Name what the tree does hold: a runner that lays the build output out
    # differently has to be diagnosable from the failure alone.
    for candidate in "$target"/build/*mozangle*; do
        if [ -e "$candidate" ]; then
            printf '  %s\n' "$candidate" >&2
        fi
    done
    exit 1
fi

echo "ANGLE from $angle"

stage=$out/axysbrowser-$version-x86_64-windows
zip=$out/axysbrowser-$version-x86_64-windows.zip

rm -rf "$stage" "$zip"
mkdir -p "$stage/lib" "$stage/share/glib-2.0/schemas"

cp "$gtk"/bin/*.dll "$stage/"
cp -r "$gtk/lib/gdk-pixbuf-2.0" "$stage/lib/"
cp "$gtk/share/glib-2.0/schemas/gschemas.compiled" "$stage/share/glib-2.0/schemas/"
cp -r "$gtk/share/icons" "$stage/share/"
cp "$exe" "$stage/axysbrowser.exe"
cp "$angle/libEGL.dll" "$angle/libGLESv2.dll" "$stage/"

# CRLF: cmd.exe still misparses `set` lines that end in a bare LF.
printf 'axysBrowser %s - Windows x86_64 (Servo engine)\r\n\r\nRun run-axysbrowser.bat, or axysbrowser.exe from this folder.\r\n\r\nThe GTK 4 runtime, its icon theme and the MSVC runtime are included, so\r\nnothing has to be installed. The launcher only exposes this folder to the\r\nloader path so the gdk-pixbuf loaders are found next to the executable.\r\n' \
    "$version" >"$stage/README.txt"

printf '@echo off\r\nset "PATH=%%~dp0;%%PATH%%"\r\nset "GDK_PIXBUF_MODULEDIR=%%~dp0lib\\gdk-pixbuf-2.0\\2.10.0\\loaders"\r\nstart "" "%%~dp0axysbrowser.exe" %%*\r\n' \
    >"$stage/run-axysbrowser.bat"

for required in \
    axysbrowser.exe \
    libEGL.dll \
    libGLESv2.dll \
    lib/gdk-pixbuf-2.0/2.10.0/loaders/pixbufloader_svg.dll \
    share/glib-2.0/schemas/gschemas.compiled \
    share/icons/Adwaita/index.theme; do
    if [ ! -f "$stage/$required" ]; then
        echo "staged payload is missing $required" >&2
        exit 1
    fi
done

ps1=$(mktemp --suffix=.ps1)
trap 'rm -f "$dll_dirs" "$ps1"' EXIT

cat >"$ps1" <<'POWERSHELL'
param([Parameter(Mandatory = $true)][string]$Dir, [int]$TimeoutSeconds = 90)

$ErrorActionPreference = 'Stop'

$exe = Join-Path $Dir 'axysbrowser.exe'
$stdout = Join-Path $env:TEMP 'axysbrowser-package-stdout.log'
$stderr = Join-Path $env:TEMP 'axysbrowser-package-stderr.log'

# Same environment the launcher sets, so the check covers the shipped layout.
$env:PATH = "$Dir;$env:PATH"
$env:GDK_PIXBUF_MODULEDIR = Join-Path $Dir 'lib\gdk-pixbuf-2.0\2.10.0\loaders'

$process = Start-Process -FilePath $exe -PassThru `
    -RedirectStandardOutput $stdout -RedirectStandardError $stderr
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$showed = $false

while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 500

    if ($process.HasExited) { break }

    $process.Refresh()

    if ($process.MainWindowHandle -ne [IntPtr]::Zero -and $process.MainWindowTitle -ne '') {
        $showed = $true
        break
    }
}

if (-not $showed) {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }

    Write-Host 'FAIL: the packaged axysbrowser.exe never showed a window'

    if (Test-Path $stderr) {
        Get-Content $stderr | Select-Object -Last 40 | ForEach-Object { Write-Host "  $_" }
    }

    exit 1
}

$title = $process.MainWindowTitle
$responding = $process.Responding
$memory = [math]::Round($process.WorkingSet64 / 1MB)

Start-Sleep -Seconds 2
$process.Refresh()

if ($process.HasExited) {
    Write-Host 'FAIL: axysbrowser.exe exited after showing its window'

    if (Test-Path $stderr) {
        Get-Content $stderr | Select-Object -Last 40 | ForEach-Object { Write-Host "  $_" }
    }

    exit 1
}

Write-Host "window '$title', responding: $responding, ${memory} MiB"
Stop-Process -Id $process.Id -Force
POWERSHELL

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w "$ps1")" \
    -Dir "$(cygpath -w "$stage")"

python - "$stage" "$zip" <<'PYTHON'
import os
import sys
import zipfile

stage, out = sys.argv[1], sys.argv[2]
entries = 0

with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
    for root, dirs, files in os.walk(stage):
        dirs.sort()

        for name in sorted(files):
            full = os.path.join(root, name)
            # Forward slashes: a zip built with backslashes unpacks badly on
            # every tool that is not Windows Explorer.
            archive.write(full, os.path.relpath(full, stage).replace("\\", "/"))
            entries += 1

print(f"{entries} entries, {os.path.getsize(out)} bytes")
PYTHON

(cd "$(dirname "$zip")" && sha256sum "$(basename "$zip")" >"$(basename "$zip").sha256")

echo
echo "zip: $(cygpath -w "$zip")"
cat "$zip.sha256"
