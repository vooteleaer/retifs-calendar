//! Calendars on retifs: the `type=event` records of ADR-0024 in any library
//! (each library holding events is a calendar: the user's own, a club's
//! family calendar), repeating events by their RFC 5545 rule, and the
//! birthdays of the contacts the user can see, shown as yearly events
//! without being copied. Attendees are contacts (`attendee:<contact id>`).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use libretifs::{AttrValue, Client, RecordView, Request, Response};

pub const ME: &str = "me";
/// What an event is (ADR-0025): the iCalendar media type.
pub const TYPE: &str = libretifs::types::EVENT;
/// A library's calendar settings (ADR-0024): it marks a calendar with no events yet.
pub const SETTINGS_TYPE: &str = "retifs.calendar";

/// How an event may repeat, as an editor offers it: (label, RFC 5545 rule).
pub const REPEATS: [(&str, &str); 5] =
    [("Never", ""), ("Every day", "FREQ=DAILY"), ("Every week", "FREQ=WEEKLY"), ("Every month", "FREQ=MONTHLY"), ("Every year", "FREQ=YEARLY")];

/// An event as an editor has it, the way it was typed: put into the event
/// with [`Event::apply_form`] (every front end the same way).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Form {
    pub title: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    pub all_day: bool,
    /// `HH:MM`.
    pub from: String,
    pub until: String,
    /// An index into [`REPEATS`].
    pub repeat: usize,
    pub location: String,
    pub note: String,
    /// The chosen contacts: (contact id, name).
    pub attendees: Vec<(String, String)>,
}

/// An event: where it lives, its record (none until saved), its values.
/// Keys this app doesn't edit (`uid`, an app's own) are kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Event {
    pub ns: String,
    pub id: Option<String>,
    pub values: BTreeMap<String, String>,
}

/// When an event is: all day (a date) or at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum When {
    Day(NaiveDate),
    At(NaiveDateTime),
}

impl When {
    pub fn parse(s: &str) -> Option<When> {
        let s = s.trim();
        if let Ok(t) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M") {
            return Some(When::At(t));
        }
        if let Ok(t) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
            return Some(When::At(t));
        }
        NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().map(When::Day)
    }

    pub fn date(self) -> NaiveDate {
        match self {
            When::Day(d) => d,
            When::At(t) => t.date(),
        }
    }

    /// For ordering: by day, all-day ones first, then by time.
    pub fn sort_key(self) -> (NaiveDate, Option<NaiveTime>) {
        match self {
            When::Day(d) => (d, None),
            When::At(t) => (t.date(), Some(t.time())),
        }
    }

    pub fn format(self) -> String {
        match self {
            When::Day(d) => d.format("%Y-%m-%d").to_string(),
            When::At(t) => t.format("%Y-%m-%dT%H:%M").to_string(),
        }
    }

    fn shift(self, days: i64, months: i32) -> Option<When> {
        let move_date = |d: NaiveDate| -> Option<NaiveDate> {
            let d = if months != 0 { add_months(d, months)? } else { d };
            d.checked_add_signed(Duration::days(days))
        };
        Some(match self {
            When::Day(d) => When::Day(move_date(d)?),
            When::At(t) => When::At(NaiveDateTime::new(move_date(t.date())?, t.time())),
        })
    }
}

/// `d` plus `n` months; None if that day doesn't exist (31 January plus a
/// month), which RFC 5545 skips.
fn add_months(d: NaiveDate, n: i32) -> Option<NaiveDate> {
    let total = d.year() * 12 + d.month0() as i32 + n;
    NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, d.day())
}

impl Event {
    pub fn new(ns: &str) -> Event {
        Event { ns: ns.into(), ..Default::default() }
    }

    fn from_record(ns: &str, r: &RecordView) -> Event {
        let values = r
            .attrs
            .iter()
            .filter_map(|(k, v)| match v {
                AttrValue::Str(s) => Some((k.clone(), s.clone())),
                _ => None,
            })
            .collect();
        Event { ns: ns.into(), id: Some(r.id.clone()), values }
    }

    pub fn get(&self, key: &str) -> &str {
        self.values.get(key).map(String::as_str).unwrap_or("")
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let value = value.trim();
        if value.is_empty() {
            self.values.remove(key);
        } else {
            self.values.insert(key.into(), value.into());
        }
    }

    pub fn title(&self) -> String {
        let t = self.get("title");
        // Without a title: one of a free/busy share, or one not named yet.
        if t.is_empty() { "Busy".into() } else { t.into() }
    }

    pub fn start(&self) -> Option<When> {
        When::parse(self.get("start"))
    }

    /// Its end: as written, else an hour after a timed start, else the next
    /// day after an all-day one (exclusive, as iCalendar).
    pub fn end(&self) -> Option<When> {
        When::parse(self.get("end")).or_else(|| match self.start()? {
            When::Day(d) => Some(When::Day(d.succ_opt()?)),
            When::At(t) => Some(When::At(t + Duration::hours(1))),
        })
    }

    /// The contacts it names as attendees: (contact id, name hint).
    pub fn attendees(&self) -> Vec<(String, String)> {
        self.values.iter().filter_map(|(k, v)| k.strip_prefix("attendee:").map(|id| (id.to_string(), v.clone()))).collect()
    }

    pub fn set_attendees(&mut self, people: &[(String, String)]) {
        self.values.retain(|k, _| !k.starts_with("attendee:"));
        for (id, name) in people {
            self.values.insert(format!("attendee:{id}"), name.clone());
        }
    }

    /// Its repetition as an index into [`REPEATS`] (a rule beyond the plain
    /// frequencies shows as its frequency).
    pub fn repeat_index(&self) -> usize {
        match Rule::parse(self.get("rrule")).freq {
            None => 0,
            Some(Freq::Daily) => 1,
            Some(Freq::Weekly) => 2,
            Some(Freq::Monthly) => 3,
            Some(Freq::Yearly) => 4,
        }
    }

    /// The editor's values put into the event. A date that doesn't parse
    /// keeps `fallback`; times that don't, 12:00 and an hour later; an end
    /// not after the start is the next day's (a night out).
    pub fn apply_form(&mut self, f: &Form, fallback: NaiveDate) {
        self.set("title", &f.title);
        let d = NaiveDate::parse_from_str(f.date.trim(), "%Y-%m-%d").unwrap_or(fallback);
        if f.all_day {
            self.set("start", &When::Day(d).format());
            self.set("end", "");
        } else {
            let t = |s: &str, dflt: (u32, u32)| s.trim().split_once(':').and_then(|(h, m)| Some((h.parse().ok()?, m.parse().ok()?))).unwrap_or(dflt);
            let (h1, m1) = t(&f.from, (12, 0));
            let (h2, m2) = t(&f.until, (h1 + 1, m1));
            let a = at(d, h1, m1);
            let mut b = at(d, h2, m2);
            if b <= a {
                b = at(d + Duration::days(1), h2, m2);
            }
            self.set("start", &a.format());
            self.set("end", &b.format());
        }
        self.set("rrule", REPEATS.get(f.repeat).map_or("", |r| r.1));
        self.set("location", &f.location);
        self.set("note", &f.note);
        self.set_attendees(&f.attendees);
    }

    /// Its start in every repetition (or once) that falls in `from..to`
    /// (one that began earlier but still runs counts). Dates a rule makes
    /// that don't exist (31 February) are skipped and not counted, as RFC
    /// 5545 says.
    pub fn starts_between(&self, from: NaiveDate, to: NaiveDate) -> Vec<When> {
        let Some(start) = self.start() else { return Vec::new() };
        let rule = Rule::parse(self.get("rrule"));
        let days = self.end().map_or(1, |e| (e.date() - start.date()).num_days().max(1));
        let step = rule.interval as i64;
        let mut out = Vec::new();
        let mut made = 0u32;
        for i in 0..100_000i64 {
            let t = match rule.freq {
                None if i > 0 => break,
                None => Some(start),
                Some(Freq::Daily) => start.shift(i * step, 0),
                Some(Freq::Weekly) => start.shift(7 * i * step, 0),
                Some(Freq::Monthly) => start.shift(0, (i * step) as i32),
                Some(Freq::Yearly) => start.shift(0, (12 * i * step) as i32),
            };
            let Some(t) = t else { continue };
            if t.date() >= to || rule.count.is_some_and(|c| made >= c) || rule.until.is_some_and(|u| t.date() > u) {
                break;
            }
            made += 1;
            if t.date() + Duration::days(days - 1) >= from {
                out.push(t);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// The parts of an RFC 5545 rule this calendar follows: FREQ, INTERVAL,
/// COUNT, UNTIL. Others (BYDAY lists, …) are kept in the record and shown
/// as the plain frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub freq: Option<Freq>,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<NaiveDate>,
}

impl Rule {
    pub fn parse(s: &str) -> Rule {
        let mut r = Rule { freq: None, interval: 1, count: None, until: None };
        for part in s.split(';') {
            let Some((k, v)) = part.split_once('=') else { continue };
            match k.trim().to_ascii_uppercase().as_str() {
                "FREQ" => {
                    r.freq = match v.trim().to_ascii_uppercase().as_str() {
                        "DAILY" => Some(Freq::Daily),
                        "WEEKLY" => Some(Freq::Weekly),
                        "MONTHLY" => Some(Freq::Monthly),
                        "YEARLY" => Some(Freq::Yearly),
                        _ => None,
                    }
                }
                "INTERVAL" => r.interval = v.trim().parse().unwrap_or(1).max(1),
                "COUNT" => r.count = v.trim().parse().ok(),
                "UNTIL" => r.until = NaiveDate::parse_from_str(&v.trim()[..8.min(v.trim().len())], "%Y%m%d").ok(),
                _ => {}
            }
        }
        r
    }
}

/// One showing of an event (or a birthday) on the calendar.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    pub event: Event,
    pub start: When,
    pub end: When,
    /// A contact's birthday, not an event record: (contact id, age that day if the year is known).
    pub birthday: Option<(String, Option<i32>)>,
}

impl Occurrence {
    pub fn title(&self) -> String {
        match &self.birthday {
            Some((_, Some(age))) => format!("{}'s birthday ({age})", self.event.title()),
            Some((_, None)) => format!("{}'s birthday", self.event.title()),
            None => self.event.title(),
        }
    }

    /// Its time for a list: "All day" or "15:00–18:00".
    pub fn time_text(&self) -> String {
        match (self.start, self.end) {
            (When::At(a), When::At(b)) if a.date() == b.date() => format!("{}–{}", a.format("%H:%M"), b.format("%H:%M")),
            (When::At(a), When::At(b)) => format!("{} – {}", a.format("%H:%M"), b.format("%d.%m %H:%M")),
            (When::At(a), _) => a.format("%H:%M").to_string(),
            _ => "All day".into(),
        }
    }
}

/// A contact the calendar knows: for attendees and birthdays.
#[derive(Debug, Clone, PartialEq)]
pub struct Person {
    pub ns: String,
    pub id: String,
    pub name: String,
    /// `YYYY-MM-DD` or `--MM-DD`.
    pub birthday: String,
}

impl Person {
    /// The birthday in year `y`: (date, age if the birth year is known).
    pub fn birthday_in(&self, y: i32) -> Option<(NaiveDate, Option<i32>)> {
        let b = self.birthday.trim();
        let (year, md) = if let Some(md) = b.strip_prefix("--") { (None, md) } else { (b.get(..4)?.parse::<i32>().ok(), b.get(5..)?) };
        let (m, d) = md.split_once('-')?;
        let (m, d) = (m.parse().ok()?, d.parse().ok()?);
        // 29 February in a year without it: the 28th.
        let date = NaiveDate::from_ymd_opt(y, m, d).or_else(|| NaiveDate::from_ymd_opt(y, m, d - 1))?;
        Some((date, year.map(|by| y - by)))
    }
}

/// The connection to `retifsd`.
pub struct Calendar {
    rt: tokio::runtime::Runtime,
    store: PathBuf,
}

impl Default for Calendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Calendar {
    pub fn new() -> Self {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a tokio runtime");
        Calendar { rt, store: libretifs::default_store_dir() }
    }

    fn client(&self) -> Result<Client, String> {
        self.rt.block_on(Client::connect(&self.store)).map_err(|e| e.to_string())
    }

    fn call(&self, c: &mut Client, req: Request) -> Result<Response, String> {
        match self.rt.block_on(c.call(req)).map_err(|e| e.to_string())? {
            Response::Error { message } => Err(message),
            r => Ok(r),
        }
    }

    fn query(&self, c: &mut Client, ns: &str, terms: Vec<String>) -> Vec<RecordView> {
        match self.call(c, Request::Query { ns: ns.into(), terms, at: None }) {
            Ok(Response::Records { records }) => records,
            _ => Vec::new(),
        }
    }

    /// The libraries this device has: `me` first, then by name.
    pub fn libraries(&self) -> Result<Vec<String>, String> {
        let mut c = self.client()?;
        let mut names = match self.call(&mut c, Request::Namespaces)? {
            Response::Names { names } => names,
            _ => Vec::new(),
        };
        names.retain(|n| n != ME && !n.starts_with("ch-"));
        // Shares this node keeps for others (a calendar's free/busy view, one
        // shared event) aren't calendars of its own: their source is.
        let shares: Vec<String> = names
            .iter()
            .flat_map(|ns| self.query(&mut c, ns, vec!["type=retifs.share".into()]))
            .filter_map(|r| r.attrs.iter().find(|(k, _)| k == "name").and_then(|(_, v)| if let AttrValue::Str(s) = v { Some(s.clone()) } else { None }))
            .collect();
        names.retain(|n| !shares.contains(n));
        // A library is a calendar if it holds events, or says it is one
        // (a new, empty calendar). Libraries of other kinds (photos, an
        // address book) are left to the apps for them.
        names.retain(|n| {
            libretifs::types::terms(TYPE).into_iter().any(|t| !self.query(&mut c, n, vec![t]).is_empty()) || !self.query(&mut c, n, vec![format!("type={SETTINGS_TYPE}")]).is_empty()
        });
        names.sort_by_key(|n| n.to_lowercase());
        names.insert(0, ME.into());
        Ok(names)
    }

    /// The events of library `ns` as they are (a repeating one once, with its
    /// rule): what a system calendar a sync adapter keeps takes.
    pub fn events(&self, ns: &str) -> Result<Vec<Event>, String> {
        let mut c = self.client()?;
        Ok(libretifs::types::terms(TYPE).into_iter().flat_map(|t| self.query(&mut c, ns, vec![t])).map(|r| Event::from_record(ns, &r)).collect())
    }

    /// Library `ns`'s colour as a calendar (its `retifs.calendar` record), if it has one.
    pub fn color(&self, ns: &str) -> Option<String> {
        let mut c = self.client().ok()?;
        self.query(&mut c, ns, vec![format!("type={SETTINGS_TYPE}")]).iter().find_map(|r| {
            r.attrs.iter().find(|(k, _)| k == "color").and_then(|(_, v)| if let AttrValue::Str(s) = v { Some(s.clone()) } else { None })
        })
    }

    /// The events and birthdays in `libraries` from `from` until `to` (exclusive), by start.
    pub fn occurrences(&self, libraries: &[String], from: NaiveDate, to: NaiveDate, birthdays: bool) -> Result<Vec<Occurrence>, String> {
        let mut c = self.client()?;
        let mut out = Vec::new();
        for ns in libraries {
            let records: Vec<RecordView> = libretifs::types::terms(TYPE).into_iter().flat_map(|t| self.query(&mut c, ns, vec![t])).collect();
            for r in records {
                let e = Event::from_record(ns, &r);
                let (Some(s), Some(en)) = (e.start(), e.end()) else { continue };
                let length = |w: When| match (s, en, w) {
                    (When::At(a), When::At(b), When::At(t)) => When::At(t + (b - a)),
                    (_, _, w) => w.shift((en.date() - s.date()).num_days(), 0).unwrap_or(w),
                };
                for start in e.starts_between(from, to) {
                    out.push(Occurrence { event: e.clone(), start, end: length(start), birthday: None });
                }
            }
            if birthdays {
                for p in self.people_in(&mut c, ns).into_iter().filter(|p| !p.birthday.is_empty()) {
                    for y in from.year()..=to.year() {
                        let Some((d, age)) = p.birthday_in(y) else { continue };
                        if d >= from && d < to {
                            let mut e = Event::new(ns);
                            e.set("title", &p.name);
                            out.push(Occurrence { event: e, start: When::Day(d), end: When::Day(d + Duration::days(1)), birthday: Some((p.id.clone(), age)) });
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| a.start.sort_key().cmp(&b.start.sort_key()).then(a.title().cmp(&b.title())));
        Ok(out)
    }

    fn people_in(&self, c: &mut Client, ns: &str) -> Vec<Person> {
        libretifs::types::terms(libretifs::types::PERSON)
            .into_iter()
            .flat_map(|t| self.query(c, ns, vec![t]))
            .collect::<Vec<_>>()
            .iter()
            .map(|r| {
                let get = |k: &str| r.attrs.iter().find(|(key, _)| key == k).and_then(|(_, v)| if let AttrValue::Str(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
                let full = [get("given"), get("family")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
                let name = [get("name"), full, get("email")].into_iter().find(|s| !s.is_empty()).unwrap_or_else(|| "No name".into());
                Person { ns: ns.into(), id: r.id.clone(), name, birthday: get("birthday") }
            })
            .collect()
    }

    /// The contacts in `libraries` (for attendees), by name.
    pub fn people(&self, libraries: &[String]) -> Vec<Person> {
        let Ok(mut c) = self.client() else { return Vec::new() };
        let mut out: Vec<Person> = libraries.iter().flat_map(|ns| self.people_in(&mut c, ns)).collect();
        out.sort_by_key(|p| p.name.to_lowercase());
        out
    }

    /// Saves an event: a new record, or what changed (keys it no longer has are unset).
    pub fn save(&self, e: &Event, before: Option<&Event>) -> Result<Event, String> {
        let mut c = self.client()?;
        let attrs: Vec<(String, AttrValue)> =
            e.values.iter().filter(|(k, v)| before.is_none_or(|b| b.values.get(*k) != Some(*v))).map(|(k, v)| (k.clone(), AttrValue::Str(v.clone()))).collect();
        let req = match &e.id {
            Some(id) => {
                let unset = before.map(|b| b.values.keys().filter(|k| !e.values.contains_key(*k)).cloned().collect()).unwrap_or_default();
                Request::RecordSet { ns: e.ns.clone(), id: id.clone(), attrs, unset, data: None }
            }
            None => {
                let mut attrs = attrs;
                attrs.push(("type".into(), AttrValue::Str(TYPE.into())));
                Request::RecordCreate { ns: e.ns.clone(), attrs, data: None }
            }
        };
        match self.call(&mut c, req)? {
            Response::Records { records } => records.first().map(|r| Event::from_record(&e.ns, r)).ok_or_else(|| "nothing saved".into()),
            other => Err(format!("unexpected answer: {other:?}")),
        }
    }

    pub fn delete(&self, e: &Event) -> Result<(), String> {
        let Some(id) = &e.id else { return Ok(()) };
        let mut c = self.client()?;
        self.call(&mut c, Request::RecordDelete { ns: e.ns.clone(), id: id.clone() }).map(|_| ())
    }

    /// A `retifs:` link to read library `ns` (a calendar, or a share made from one).
    fn read_link(&self, c: &mut Client, ns: &str, label: &str) -> Result<String, String> {
        let address = match self.call(c, Request::NsAddress { ns: ns.into() })? {
            Response::Channel { address } => address,
            other => return Err(format!("unexpected answer: {other:?}")),
        };
        let (node, _, _) = self.rt.block_on(c.ping()).map_err(|e| e.to_string())?;
        // `view=calendar` (ADR-0020): whatever opens the link joins the
        // library and opens it in a calendar.
        let mut link = libretifs::link::Link::namespace(&address, &[node], Some(label));
        link.params.push(("view".into(), "calendar".into()));
        Ok(link.to_string())
    }

    /// An invitation to calendar `ns` (ADR-0020): whoever opens it first may
    /// read and write it, as a member (a colleague joining the work calendar).
    pub fn invite_link(&self, ns: &str) -> Result<String, String> {
        let mut c = self.client()?;
        let req = Request::InviteCreate { ns: ns.into(), read: false, grant: false, keys: vec![], until: None, expires_ms: Some(7 * 86_400_000), q: None, label: Some(ns.into()) };
        match self.call(&mut c, req)? {
            Response::Link { link } => Ok(link),
            other => Err(format!("unexpected answer: {other:?}")),
        }
    }

    /// A link to read calendar `ns`: all of it, or with `free_busy` only when
    /// its events are (a share without titles, places, notes or attendees,
    /// ADR-0021: a partner finding a time).
    pub fn share_calendar(&self, ns: &str, free_busy: bool) -> Result<String, String> {
        let mut c = self.client()?;
        if !free_busy {
            return self.read_link(&mut c, ns, ns);
        }
        let name = format!("{ns}-busy");
        let omit = ["title", "note", "location", "attendee:*", "uid"].map(String::from).to_vec();
        let req = Request::ShareCreate { source: ns.into(), name: name.clone(), terms: vec![format!("type={TYPE}")], ids: vec![], omit };
        match self.call(&mut c, req) {
            Ok(_) => {}
            // Made before: its link again.
            Err(e) if e.contains("already exists") => {}
            Err(e) => return Err(e),
        }
        self.read_link(&mut c, &name, &format!("{ns} (free/busy)"))
    }

    /// A link to read one event (a share of just it, kept up to date: a
    /// client sees the meeting move when it moves).
    pub fn share_event(&self, e: &Event) -> Result<String, String> {
        let id = e.id.clone().ok_or("save the event first")?;
        let mut c = self.client()?;
        let slug: String = e.title().to_lowercase().chars().map(|ch| if ch.is_alphanumeric() { ch } else { '-' }).collect::<String>().split('-').filter(|s| !s.is_empty()).take(4).collect::<Vec<_>>().join("-");
        let name = format!("{}-{}-{}", e.ns, if slug.is_empty() { "event" } else { &slug }, &id[..6]);
        let req = Request::ShareCreate { source: e.ns.clone(), name: name.clone(), terms: vec![], ids: vec![id], omit: vec![] };
        match self.call(&mut c, req) {
            Ok(_) => {}
            Err(err) if err.contains("already exists") => {}
            Err(err) => return Err(err),
        }
        self.read_link(&mut c, &name, &e.title())
    }

    /// Each library's change count: a front end reloads when these differ.
    pub fn change_counts(&self, libraries: &[String]) -> HashMap<String, u64> {
        let Ok(mut c) = self.client() else { return HashMap::new() };
        libraries
            .iter()
            .filter_map(|ns| match self.rt.block_on(c.call(Request::Watch { ns: ns.clone(), since: 0, timeout_ms: 0 })) {
                Ok(Response::Changed { count }) => Some((ns.clone(), count)),
                _ => None,
            })
            .collect()
    }

    /// Waits until one of the libraries in `known` (with their change counts)
    /// changes, for at most `timeout`: whether one did, its count in `known`
    /// brought up to date. retifsd answers the moment a change comes, so a
    /// front end waiting on this (on a thread, with a `Calendar` of its own)
    /// shows an event at once instead of asking every few seconds.
    pub fn wait_change(&self, known: &mut HashMap<String, u64>, timeout: std::time::Duration) -> bool {
        let watches: Vec<_> = known.iter().filter_map(|(ns, n)| self.client().ok().map(|c| (c, ns.clone(), *n))).collect();
        if watches.is_empty() {
            std::thread::sleep(timeout.min(std::time::Duration::from_secs(5)));
            return false;
        }
        match self.rt.block_on(libretifs::watch_any(watches, timeout)) {
            Ok(Some((ns, count))) => {
                known.insert(ns, count);
                // Changes come in bursts (a sync merging many): a second
                // more, then every count as it is, so one reload takes them all.
                std::thread::sleep(std::time::Duration::from_secs(1));
                let libraries: Vec<String> = known.keys().cloned().collect();
                known.extend(self.change_counts(&libraries));
                true
            }
            Ok(None) => false,
            // retifsd away (restarting): not at once again.
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_secs(5));
                false
            }
        }
    }
}

/// `date` at `hh:mm`.
pub fn at(date: NaiveDate, hh: u32, mm: u32) -> When {
    When::At(NaiveDateTime::new(date, NaiveTime::from_hms_opt(hh, mm, 0).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn ev(start: &str, rrule: &str) -> Event {
        let mut e = Event::new(ME);
        e.set("start", start);
        e.set("rrule", rrule);
        e
    }

    #[test]
    fn once_and_spans() {
        let e = ev("2026-10-24T15:00", "");
        assert_eq!(e.starts_between(d(2026, 10, 1), d(2026, 11, 1)), [When::parse("2026-10-24T15:00").unwrap()]);
        assert!(e.starts_between(d(2026, 10, 25), d(2026, 11, 1)).is_empty());
        assert_eq!(e.end(), When::parse("2026-10-24T16:00"));
        let mut trip = ev("2026-10-20", "");
        trip.set("end", "2026-10-27");
        assert_eq!(trip.starts_between(d(2026, 10, 24), d(2026, 10, 25)).len(), 1, "still running on the 24th");
    }

    #[test]
    fn repeating() {
        let weekly = ev("2026-10-05T18:00", "FREQ=WEEKLY");
        assert_eq!(weekly.starts_between(d(2026, 10, 1), d(2026, 11, 1)).len(), 4);
        let fortnight = ev("2026-10-05", "FREQ=WEEKLY;INTERVAL=2;COUNT=3");
        assert_eq!(fortnight.starts_between(d(2026, 1, 1), d(2027, 1, 1)).iter().map(|w| w.format()).collect::<Vec<_>>(), ["2026-10-05", "2026-10-19", "2026-11-02"]);
        let monthly31 = ev("2026-01-31", "FREQ=MONTHLY");
        let got: Vec<String> = monthly31.starts_between(d(2026, 1, 1), d(2026, 6, 1)).iter().map(|w| w.format()).collect();
        assert_eq!(got, ["2026-01-31", "2026-03-31", "2026-05-31"], "months without a 31st are skipped");
        let yearly = ev("2024-02-29", "FREQ=YEARLY;UNTIL=20330101");
        assert_eq!(yearly.starts_between(d(2024, 1, 1), d(2040, 1, 1)).len(), 3, "2024, 2028, 2032");
        let daily = ev("2026-10-30", "FREQ=DAILY");
        assert_eq!(daily.starts_between(d(2026, 10, 1), d(2026, 11, 3)).len(), 4);
    }

    #[test]
    fn birthdays_and_attendees() {
        let p = Person { ns: ME.into(), id: "a".into(), name: "Ema".into(), birthday: "1961-03-14".into() };
        assert_eq!(p.birthday_in(2027), Some((d(2027, 3, 14), Some(66))));
        let leap = Person { birthday: "--02-29".into(), ..p.clone() };
        assert_eq!(leap.birthday_in(2027), Some((d(2027, 2, 28), None)));
        let mut e = Event::new(ME);
        e.set_attendees(&[("abc".into(), "Ema".into())]);
        assert_eq!(e.attendees(), [("abc".to_string(), "Ema".to_string())]);
        e.set_attendees(&[]);
        assert!(e.attendees().is_empty());
        let mut list = [When::parse("2026-10-25T17:00").unwrap(), When::parse("2026-10-28").unwrap(), When::parse("2026-10-25").unwrap()];
        list.sort_by_key(|w| w.sort_key());
        assert_eq!(list.map(|w| w.format()), ["2026-10-25", "2026-10-25T17:00", "2026-10-28"]);
    }

    #[test]
    fn form_into_event() {
        let fallback = d(2026, 11, 2);
        let mut e = Event::new(ME);
        let f = Form {
            title: "Sauna".into(),
            date: "2026-11-06".into(),
            from: "22:00".into(),
            until: "01:30".into(),
            repeat: 2,
            location: "Tartu".into(),
            attendees: vec![("c1".into(), "Mari".into())],
            ..Default::default()
        };
        e.apply_form(&f, fallback);
        assert_eq!((e.get("start"), e.get("end"), e.get("rrule")), ("2026-11-06T22:00", "2026-11-07T01:30", "FREQ=WEEKLY"));
        assert_eq!(e.attendees(), vec![("c1".to_string(), "Mari".to_string())]);
        assert_eq!(e.repeat_index(), 2);
        // All day, and a date that doesn't parse: the fallback day, no end.
        e.apply_form(&Form { title: "Trip".into(), date: "soon".into(), all_day: true, ..Default::default() }, fallback);
        assert_eq!((e.get("start"), e.get("end"), e.get("location"), e.repeat_index()), ("2026-11-02", "", "", 0));
        assert!(e.attendees().is_empty());
    }
}
