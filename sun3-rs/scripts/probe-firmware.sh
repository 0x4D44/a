#!/usr/bin/env bash
set -euo pipefail
OUT=${1:-build/firmware}
mkdir -p "$OUT/unpacked"
URL=https://oldsilicon.com/technologies/sun-rom-images/ROMS/3.60_v3.0.1.zip
ZIP="$OUT/3.60_v3.0.1.zip"
curl --fail --location --retry 4 --retry-delay 2 "$URL" -o "$ZIP"
{
  echo "source=$URL"
  echo "downloaded=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  sha256sum "$ZIP"
  sha1sum "$ZIP"
  unzip -l "$ZIP"
} | tee "$OUT/probe.txt"
unzip -o "$ZIP" -d "$OUT/unpacked"
find "$OUT/unpacked" -type f -print0 | sort -z | while IFS= read -r -d '' file; do
  {
    printf '\nfile=%q\n' "$file"
    stat --printf='size=%s\n' "$file"
    sha256sum "$file"
    sha1sum "$file"
    file "$file"
    xxd -l 64 "$file"
  } | tee -a "$OUT/probe.txt"
done
