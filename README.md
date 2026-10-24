# retifs Calendar

Calendars on [retifs](../retifs): the `type=event` records of ADR-0024. Each library holding events is a calendar: your own (`me`), a family's (a club's library, shared with its members), work's.

- A month to pick a day from (days with something on are marked), the calendars to show, and the day's events with what's coming up in the next two weeks.
- Events: title, all day or from–until, where, a note, repeating (every day, week, month or year; RFC 5545 rules, months without the day skipped), and who: attendees are your contacts (`attendee:<contact id>`), and their cards in retifs Contacts list the events they're at.
- Birthdays come from the contacts you can see, shown yearly with the age when the year is known, never copied into events.
- Updates as the libraries change, here or on another device.

`core/` talks to `retifsd` through libretifs; `gtk/` is the Linux front end (GTK 4, libadwaita). Build on Linux (or WSL): `cargo build -p retifs-calendar-gtk`. For demos, `RETIFS_CALENDAR_DATE=YYYY-MM-DD` opens a day and `RETIFS_CALENDAR_SNAPSHOT=<file.png>` draws the window to a file every 2 s.
