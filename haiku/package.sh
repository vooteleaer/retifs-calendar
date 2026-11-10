#!/bin/sh
# Makes the retifs Calendar package for Haiku (an hpkg) from the app folder
# build.sh made: the app with its library in apps/retifs_calendar, and an
# entry in the Deskbar's Applications menu. retifs itself (the daemon) comes
# from the retifs package.
#
#     haiku/build.sh && haiku/package.sh
#     pkgman install haiku/build/retifs_calendar-<version>-x86_64.hpkg
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/.." && pwd)
APP=$HERE/build/Calendar
STAGE=$HERE/build/package
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)-3
ARCH=x86_64

for f in Calendar lib/libretifs_calendar.so; do
	[ -f "$APP/$f" ] || { echo "missing $APP/$f: build it first" >&2; exit 1; }
done

rm -rf "${STAGE:?}"
mkdir -p "$STAGE/apps" "$STAGE/data/deskbar/menu/Applications"
cp -a "$APP" "$STAGE/apps/retifs_calendar"
ln -s ../../../../apps/retifs_calendar/Calendar \
	"$STAGE/data/deskbar/menu/Applications/retifs Calendar"

cat > "$STAGE/.PackageInfo" <<EOF
name			retifs_calendar
version			$VERSION
architecture	$ARCH
summary			"Calendars on retifs"
description		"Calendars on retifs: each library holding events is a calendar (your own, \
a family's, work's). Events with times, places, notes and repeats, attendees \
from your contacts and their birthdays, and sharing a calendar (a member, a \
reader, free/busy) or one event. A working prototype."
packager		"retifs <https://github.com/vooteleaer/retifs>"
vendor			"retifs"
licenses {
	"MIT"
}
copyrights {
	"2026 retifs contributors"
}
provides {
	retifs_calendar = $VERSION
	app:retifs_calendar = $VERSION
}
requires {
	haiku
	retifs
	lib:libgcc_s
	lib:libstdc++
}
urls {
	"https://github.com/vooteleaer/retifs"
}
EOF

PKG=$HERE/build/retifs_calendar-$VERSION-$ARCH.hpkg
rm -f "$PKG"
package create -C "$STAGE" "$PKG"
echo "== $PKG"
package list "$PKG" | tail -12
