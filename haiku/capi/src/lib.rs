//! retifs Calendar for Haiku: the shared core to C (and so to the BeAPI
//! front end), JSON in and out. The same calls as the GTK front end makes.
//!
//! Every returned string is the caller's to free with `rc_free`. Calls
//! block: the front end makes them off its window threads.
//!
//! An event as JSON: `{"ns": "me", "id": "…" | null, "values": {"title": …}}`.

use std::collections::{BTreeMap, HashMap};
use std::ffi::{CStr, CString, c_char, c_int};

use chrono::{Duration, NaiveDate};
use retifs_calendar_core::{Calendar, Event, Form, Occurrence, REPEATS};
use serde_json::{Value, json};

fn out(v: Value) -> *mut c_char {
    CString::new(v.to_string().replace('\0', " ")).expect("no NUL left").into_raw()
}

/// # Safety
/// `s` is null or a NUL-terminated string.
unsafe fn arg(s: *const c_char) -> String {
    if s.is_null() { String::new() } else { unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned() }
}

fn strings(json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(json).unwrap_or_default()
}

fn date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

fn event_json(e: &Event) -> Value {
    json!({ "ns": e.ns, "id": e.id, "values": e.values })
}

fn event_from(v: &Value) -> Option<Event> {
    let values: BTreeMap<String, String> = v["values"]
        .as_object()
        .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
        .unwrap_or_default();
    Some(Event { ns: v["ns"].as_str()?.to_string(), id: v["id"].as_str().map(str::to_string), values })
}

fn occurrence_json(o: &Occurrence) -> Value {
    let first = o.start.date();
    // The days it is on: from its start until (exclusive) its end, at least one.
    let until = o.end.date().max(first + Duration::days(1));
    let people: Vec<String> = o.event.attendees().into_iter().map(|(_, n)| n).collect();
    json!({
        "title": o.title(),
        "time": o.time_text(),
        "first": first.format("%Y-%m-%d").to_string(),
        "until": until.format("%Y-%m-%d").to_string(),
        "weekday": first.format("%a %-d %b").to_string(),
        "birthday": o.birthday.is_some(),
        "repeats": !o.event.get("rrule").is_empty(),
        "location": o.event.get("location"),
        "people": people.join(", "),
        "ns": o.event.ns,
        "event_json": event_json(&o.event).to_string(),
    })
}

fn error(e: impl std::fmt::Display) -> Value {
    json!({ "error": e.to_string() })
}

/// The calendars (libraries with events): `me` first (JSON array).
#[unsafe(no_mangle)]
pub extern "C" fn rc_libraries() -> *mut c_char {
    out(Calendar::new().libraries().map(|l| json!(l)).unwrap_or_else(error))
}

/// The events and birthdays in `libraries` (a JSON array of names) from
/// `from` until `to` (exclusive, `YYYY-MM-DD`), by start (JSON array).
///
/// # Safety
/// The strings are NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_occurrences(libraries: *const c_char, from: *const c_char, to: *const c_char, birthdays: c_int) -> *mut c_char {
    let (libs, from, to) = unsafe { (strings(&arg(libraries)), arg(from), arg(to)) };
    let (Some(from), Some(to)) = (date(&from), date(&to)) else { return out(error("dates are YYYY-MM-DD")) };
    out(match Calendar::new().occurrences(&libs, from, to, birthdays != 0) {
        Ok(list) => Value::Array(list.iter().map(occurrence_json).collect()),
        Err(e) => error(e),
    })
}

/// The contacts in `libraries`, for attendees: `[{"ns", "id", "name"}]`.
///
/// # Safety
/// `libraries` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_people(libraries: *const c_char) -> *mut c_char {
    let libs = strings(&unsafe { arg(libraries) });
    let people: Vec<Value> = Calendar::new().people(&libs).iter().map(|p| json!({ "ns": p.ns, "id": p.id, "name": p.name })).collect();
    out(Value::Array(people))
}

/// The repeat choices an editor offers (labels, in order: a form's `repeat`).
#[unsafe(no_mangle)]
pub extern "C" fn rc_repeats() -> *mut c_char {
    out(json!(REPEATS.iter().map(|(label, _)| *label).collect::<Vec<_>>()))
}

/// What the editor shows for an event: its fields as text
/// `{"title", "date", "all_day", "from", "until", "repeat", "location", "note", "attendees": [id]}`.
///
/// # Safety
/// `event` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_form_of(event: *const c_char, day: *const c_char) -> *mut c_char {
    let v: Value = serde_json::from_str(&unsafe { arg(event) }).unwrap_or(Value::Null);
    let Some(e) = event_from(&v) else { return out(error("not an event")) };
    let day = date(&unsafe { arg(day) }).unwrap_or_else(|| chrono::Local::now().date_naive());
    let start = e.start().unwrap_or(retifs_calendar_core::When::Day(day));
    let end = e.end().unwrap_or(start);
    let hm = |w: retifs_calendar_core::When, default: &str| match w {
        retifs_calendar_core::When::At(t) => t.format("%H:%M").to_string(),
        retifs_calendar_core::When::Day(_) => default.to_string(),
    };
    out(json!({
        "title": e.get("title"),
        "date": start.date().format("%Y-%m-%d").to_string(),
        "all_day": matches!(start, retifs_calendar_core::When::Day(_)),
        "from": hm(start, "12:00"),
        "until": hm(end, "13:00"),
        "repeat": e.repeat_index(),
        "location": e.get("location"),
        "note": e.get("note"),
        "attendees": e.attendees().into_iter().map(|(id, _)| id).collect::<Vec<_>>(),
    }))
}

/// A new event in `ns` on `day` at noon (JSON).
///
/// # Safety
/// The strings are NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_new_event(ns: *const c_char, day: *const c_char) -> *mut c_char {
    let (ns, day) = unsafe { (arg(ns), arg(day)) };
    let day = date(&day).unwrap_or_else(|| chrono::Local::now().date_naive());
    let mut e = Event::new(&ns);
    e.set("start", &retifs_calendar_core::at(day, 12, 0).format());
    out(event_json(&e))
}

/// Saves the editor's form into `event` (as it was shown, its changes
/// relative to that): `{"event_json": "…", "date"}` saved, or `{"error"}`. The form as
/// [`rc_form_of`] gives it, plus `"ns"` for a new event and `"attendees":
/// [[id, name]]`.
///
/// # Safety
/// The strings are NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_save(event: *const c_char, form: *const c_char) -> *mut c_char {
    let (ev, f) = unsafe { (arg(event), arg(form)) };
    let (Ok(ev), Ok(f)) = (serde_json::from_str::<Value>(&ev), serde_json::from_str::<Value>(&f)) else { return out(error("bad JSON")) };
    let Some(before) = event_from(&ev) else { return out(error("not an event")) };
    let s = |k: &str| f[k].as_str().unwrap_or("").to_string();
    let form = Form {
        title: s("title"),
        date: s("date"),
        all_day: f["all_day"].as_bool().unwrap_or(false),
        from: s("from"),
        until: s("until"),
        repeat: f["repeat"].as_u64().unwrap_or(0) as usize,
        location: s("location"),
        note: s("note"),
        attendees: f["attendees"]
            .as_array()
            .map(|a| a.iter().filter_map(|p| Some((p[0].as_str()?.to_string(), p[1].as_str()?.to_string()))).collect())
            .unwrap_or_default(),
    };
    let mut e = before.clone();
    if e.id.is_none() && !s("ns").is_empty() {
        e.ns = s("ns");
    }
    let fallback = before.start().map_or_else(|| chrono::Local::now().date_naive(), |w| w.date());
    e.apply_form(&form, fallback);
    let previous = before.id.is_some().then_some(&before);
    out(match Calendar::new().save(&e, previous) {
        Ok(saved) => json!({ "event_json": event_json(&saved).to_string(), "date": saved.start().map(|w| w.date().format("%Y-%m-%d").to_string()) }),
        Err(err) => error(err),
    })
}

/// Deletes an event: "" or what went wrong (JSON string).
///
/// # Safety
/// `event` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_delete(event: *const c_char) -> *mut c_char {
    let v: Value = serde_json::from_str(&unsafe { arg(event) }).unwrap_or(Value::Null);
    let Some(e) = event_from(&v) else { return out(error("not an event")) };
    out(match Calendar::new().delete(&e) {
        Ok(()) => json!({}),
        Err(err) => error(err),
    })
}

fn link(r: Result<String, String>) -> *mut c_char {
    out(match r {
        Ok(l) => json!({ "link": l }),
        Err(e) => error(e),
    })
}

/// An invitation to calendar `ns` (a member reads and writes it): `{"link"}` or `{"error"}`.
///
/// # Safety
/// `ns` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_invite(ns: *const c_char) -> *mut c_char {
    link(Calendar::new().invite_link(&unsafe { arg(ns) }))
}

/// A link to read calendar `ns`: all of it, or only when it's busy.
///
/// # Safety
/// `ns` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_share_calendar(ns: *const c_char, free_busy: c_int) -> *mut c_char {
    link(Calendar::new().share_calendar(&unsafe { arg(ns) }, free_busy != 0))
}

/// A link to one event alone, kept up to date.
///
/// # Safety
/// `event` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_share_event(event: *const c_char) -> *mut c_char {
    let v: Value = serde_json::from_str(&unsafe { arg(event) }).unwrap_or(Value::Null);
    let Some(e) = event_from(&v) else { return out(error("not an event")) };
    link(Calendar::new().share_event(&e))
}

/// Each library's change count `{ns: count}`: reload when these differ.
///
/// # Safety
/// `libraries` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_change_counts(libraries: *const c_char) -> *mut c_char {
    let libs = strings(&unsafe { arg(libraries) });
    out(json!(Calendar::new().change_counts(&libs)))
}

/// Waits until one of the libraries in `known` (`{ns: count}`, the counts
/// as last loaded) changes, for at most `timeout_ms`: `{"changed", "known"}`,
/// the counts brought up to date. Blocks that long, so it is called from a
/// thread of its own, which is answered the moment retifsd sees a change.
///
/// # Safety
/// `known` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_wait_change(known: *const c_char, timeout_ms: c_int) -> *mut c_char {
    let given: HashMap<String, Value> = serde_json::from_str(&unsafe { arg(known) }).unwrap_or_default();
    // Counts through BJson come as doubles: whole numbers either way.
    let mut known: HashMap<String, u64> = given.into_iter().filter_map(|(ns, n)| Some((ns, n.as_u64().or_else(|| n.as_f64().map(|f| f as u64))?))).collect();
    let changed = Calendar::new().wait_change(&mut known, std::time::Duration::from_millis(timeout_ms.max(0) as u64));
    out(json!({ "changed": changed, "known": known }))
}

/// Frees a string this library returned.
///
/// # Safety
/// `s` came from this library and isn't used afterwards (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rc_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}
