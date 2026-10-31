#!/bin/sh
# Builds retifs Calendar for Haiku on Haiku: the core as a library
# (haiku/capi), the BeAPI app over it, as an app folder
# (haiku/build/Calendar: the app, lib/libretifs_calendar.so).
#
#     haiku/build.sh
#
# Needs: pkgman install rust_bin gcc haiku_devel; retifs next to retifs-calendar.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/.." && pwd)
OUT=$HERE/build/Calendar
cd "$ROOT"
cargo build --release --ignore-rust-version -p retifs-calendar-capi
mkdir -p "$OUT/lib"
# A new file, not the old one overwritten: a running app has it mapped.
rm -f "${OUT:?}/lib/libretifs_calendar.so"
cp target/release/libretifs_calendar.so "$OUT/lib/"
g++ -O2 -std=c++17 -Wall -o "$OUT/Calendar" "$HERE/app/CalendarApp.cpp" \
	-I "$HERE/capi" -I /boot/system/develop/headers/private/shared \
	-L "$OUT/lib" -lretifs_calendar -lbe -lshared \
	-Wl,-rpath,'$ORIGIN/lib'
rc -o "$HERE/build/calendar.rsrc" "$HERE/app/calendar.rdef"
xres -o "$OUT/Calendar" "$HERE/build/calendar.rsrc"
mimeset -f "$OUT/Calendar"
echo "== $OUT"
ls -la "$OUT" "$OUT/lib"
