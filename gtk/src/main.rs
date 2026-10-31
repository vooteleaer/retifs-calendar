//! retifs Calendar for Linux: native GTK 4 / libadwaita over the shared core.
//! A month to pick a day from, the calendars (libraries) to show, the day's
//! events and what's coming up, and an editor for an event: when, where,
//! repeating, and who (attendees are contacts, ADR-0024). Birthdays come
//! from the contacts, not copies. It updates as the libraries change.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use chrono::{Datelike, Duration, Local, NaiveDate};
use gtk::{gio, glib};
use retifs_calendar_core::{Calendar, Event, Form, Occurrence, Person, When, ME, REPEATS};

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id("org.retifs.Calendar").build();
    app.connect_activate(build);
    app.run()
}

fn bg<T: Send + 'static>(cal: &Arc<Calendar>, work: impl FnOnce(&Calendar) -> T + Send + 'static, then: impl FnOnce(T) + 'static) {
    let cal = cal.clone();
    glib::spawn_future_local(async move {
        if let Ok(r) = gio::spawn_blocking(move || work(&cal)).await {
            then(r);
        }
    });
}

struct App {
    cal: Arc<Calendar>,
    window: adw::ApplicationWindow,
    month: gtk::Calendar,
    shown: RefCell<Vec<(String, gtk::CheckButton)>>,
    calendars: gtk::ListBox,
    birthdays: gtk::Switch,
    title: adw::WindowTitle,
    day: adw::PreferencesGroup,
    coming: adw::PreferencesGroup,
    rows: RefCell<Vec<adw::ActionRow>>,
    libraries: RefCell<Vec<String>>,
    counts: RefCell<HashMap<String, u64>>,
}

fn build(gapp: &adw::Application) {
    let cal = Arc::new(Calendar::new());

    let month = gtk::Calendar::new();
    month.set_margin_start(6);
    month.set_margin_end(6);
    let calendars = gtk::ListBox::new();
    calendars.add_css_class("boxed-list");
    calendars.set_selection_mode(gtk::SelectionMode::None);
    let birthdays = gtk::Switch::builder().active(true).valign(gtk::Align::Center).build();
    let bday_row = adw::ActionRow::builder().title("Birthdays").subtitle("from your contacts").build();
    bday_row.add_suffix(&birthdays);
    let bday_list = gtk::ListBox::new();
    bday_list.add_css_class("boxed-list");
    bday_list.set_selection_mode(gtk::SelectionMode::None);
    bday_list.append(&bday_row);
    let cal_label = gtk::Label::builder().label("Calendars").xalign(0.0).css_classes(["heading"]).build();
    let side_box = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12).margin_start(12).margin_end(12).margin_top(6).margin_bottom(12).build();
    side_box.append(&month);
    side_box.append(&cal_label);
    side_box.append(&calendars);
    side_box.append(&bday_list);
    let side_view = adw::ToolbarView::new();
    side_view.add_top_bar(&adw::HeaderBar::new());
    side_view.set_content(Some(&gtk::ScrolledWindow::builder().child(&side_box).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build()));
    let side_page = adw::NavigationPage::builder().title("Calendar").child(&side_view).build();

    let title = adw::WindowTitle::new("", "");
    let add = gtk::Button::from_icon_name("list-add-symbolic");
    add.set_tooltip_text(Some("New event on this day"));
    let today = gtk::Button::builder().label("Today").build();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&title));
    header.pack_start(&today);
    header.pack_end(&add);
    let share = gtk::Button::from_icon_name("emblem-shared-symbolic");
    share.set_tooltip_text(Some("Share a calendar"));
    header.pack_end(&share);
    let page = adw::PreferencesPage::new();
    let day = adw::PreferencesGroup::new();
    let coming = adw::PreferencesGroup::builder().title("Coming up").description("The next two weeks").build();
    page.add(&day);
    page.add(&coming);
    let content = adw::ToolbarView::new();
    content.add_top_bar(&header);
    content.set_content(Some(&page));
    let content_page = adw::NavigationPage::builder().title("Day").child(&content).build();

    let split = adw::NavigationSplitView::builder().sidebar(&side_page).content(&content_page).min_sidebar_width(300.0).build();
    let window = adw::ApplicationWindow::builder().application(gapp).title("Calendar").default_width(960).default_height(680).content(&split).build();

    let me = Rc::new(App {
        cal,
        window: window.clone(),
        month: month.clone(),
        shown: RefCell::new(Vec::new()),
        calendars,
        birthdays: birthdays.clone(),
        title,
        day,
        coming,
        rows: RefCell::new(Vec::new()),
        libraries: RefCell::new(Vec::new()),
        counts: RefCell::new(HashMap::new()),
    });

    if let Some(d) = std::env::var("RETIFS_CALENDAR_DATE").ok().and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()) {
        select(&month, d);
    }
    {
        let me = me.clone();
        month.connect_day_selected(move |_| reload(&me));
    }
    {
        let me2 = me.clone();
        month.connect_next_month(move |_| reload(&me2));
        let me2 = me.clone();
        month.connect_prev_month(move |_| reload(&me2));
    }
    {
        let me = me.clone();
        birthdays.connect_active_notify(move |_| reload(&me));
    }
    {
        let me = me.clone();
        today.connect_clicked(move |_| {
            select(&me.month, Local::now().date_naive());
            reload(&me);
        });
    }
    {
        let me = me.clone();
        share.connect_clicked(move |_| share_calendar_dialog(&me));
    }
    {
        let me = me.clone();
        add.connect_clicked(move |_| {
            let d = selected(&me.month);
            let mut e = Event::new(me.shown_libraries().first().map(String::as_str).unwrap_or(ME));
            e.set("start", &retifs_calendar_core::at(d, 12, 0).format());
            edit_event(&me, e, true);
        });
    }

    let me2 = me.clone();
    bg(&me.cal, |c| c.libraries(), move |r| {
        set_libraries(&me2, r.unwrap_or_else(|_| vec![ME.into()]));
        reload(&me2);
    });
    {
        let me = me.clone();
        glib::timeout_add_seconds_local(3, move || {
            refresh_if_changed(&me);
            glib::ControlFlow::Continue
        });
    }
    snapshot_on_request(&window);
    window.present();
}

fn selected(month: &gtk::Calendar) -> NaiveDate {
    let d = month.date();
    NaiveDate::from_ymd_opt(d.year(), d.month() as u32, d.day_of_month() as u32).unwrap_or_else(|| Local::now().date_naive())
}

fn select(month: &gtk::Calendar, d: NaiveDate) {
    if let Ok(dt) = glib::DateTime::from_local(d.year(), d.month() as i32, d.day() as i32, 12, 0, 0.0) {
        month.select_day(&dt);
    }
}

impl App {
    fn shown_libraries(&self) -> Vec<String> {
        self.shown.borrow().iter().filter(|(_, c)| c.is_active()).map(|(n, _)| n.clone()).collect()
    }
}

fn set_libraries(me: &Rc<App>, libs: Vec<String>) {
    me.calendars.remove_all();
    let mut shown = Vec::new();
    for ns in &libs {
        let check = gtk::CheckButton::builder().active(true).valign(gtk::Align::Center).build();
        let row = adw::ActionRow::builder().title(if ns == ME { "Mine (me)" } else { ns.as_str() }).activatable_widget(&check).build();
        row.add_prefix(&check);
        let me2 = me.clone();
        check.connect_toggled(move |_| reload(&me2));
        me.calendars.append(&row);
        shown.push((ns.clone(), check));
    }
    *me.shown.borrow_mut() = shown;
    *me.libraries.borrow_mut() = libs;
}

fn reload(me: &Rc<App>) {
    let day = selected(&me.month);
    me.title.set_title(&day.format("%A %-d %B %Y").to_string());
    let libs = me.shown_libraries();
    let birthdays = me.birthdays.is_active();
    // The shown month (for its marks), the day, and two weeks on.
    let first = day.with_day(1).unwrap_or(day);
    let until = (first + Duration::days(45)).max(day + Duration::days(15));
    let me2 = me.clone();
    let libs2 = libs.clone();
    bg(&me.cal, move |c| (c.occurrences(&libs2, first.min(day), until, birthdays), c.change_counts(&libs2)), move |(r, counts)| {
        *me2.counts.borrow_mut() = counts;
        let list = r.unwrap_or_default();
        me2.month.clear_marks();
        for o in &list {
            let d = o.start.date();
            if d.year() == day.year() && d.month() == day.month() {
                me2.month.mark_day(d.day());
            }
        }
        show_day(&me2, day, &list);
    });
}

fn refresh_if_changed(me: &Rc<App>) {
    let libs = me.shown_libraries();
    let me2 = me.clone();
    bg(&me.cal, move |c| c.change_counts(&libs), move |counts| {
        if !counts.is_empty() && counts != *me2.counts.borrow() {
            reload(&me2);
        }
    });
}

fn show_day(me: &Rc<App>, day: NaiveDate, list: &[Occurrence]) {
    for r in me.rows.borrow_mut().drain(..) {
        if let Some(g) = r.parent().and_then(|p| p.parent()).and_then(|p| p.parent()).and_downcast::<adw::PreferencesGroup>() {
            g.remove(&r);
        } else {
            me.day.remove(&r);
            me.coming.remove(&r);
        }
    }
    let on_day: Vec<&Occurrence> = list.iter().filter(|o| o.start.date() <= day && day < o.end.date().max(o.start.date() + Duration::days(1))).collect();
    me.day.set_title(if on_day.is_empty() { "Nothing on this day" } else { "This day" });
    for o in on_day {
        add_row(me, &me.day, o, false);
    }
    let soon: Vec<&Occurrence> = list.iter().filter(|o| o.start.date() > day && o.start.date() <= day + Duration::days(14)).collect();
    me.coming.set_visible(!soon.is_empty());
    for o in soon {
        add_row(me, &me.coming, o, true);
    }
}

fn add_row(me: &Rc<App>, group: &adw::PreferencesGroup, o: &Occurrence, with_date: bool) {
    let when = if with_date { format!("{} · {}", o.start.date().format("%a %-d %b"), o.time_text()) } else { o.time_text() };
    let mut parts = vec![when];
    if !o.event.get("location").is_empty() {
        parts.push(o.event.get("location").to_string());
    }
    let people = o.event.attendees();
    if !people.is_empty() {
        parts.push(people.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", "));
    }
    if o.event.ns != ME {
        parts.push(o.event.ns.clone());
    }
    let row = adw::ActionRow::builder().title(glib::markup_escape_text(&o.title())).subtitle(glib::markup_escape_text(&parts.join(" · "))).build();
    let icon = if o.birthday.is_some() { "starred-symbolic" } else if !o.event.get("rrule").is_empty() { "media-playlist-repeat-symbolic" } else { "x-office-calendar-symbolic" };
    row.add_prefix(&gtk::Image::from_icon_name(icon));
    if o.birthday.is_none() {
        row.set_activatable(true);
        let (me2, e) = (me.clone(), o.event.clone());
        row.connect_activated(move |_| edit_event(&me2, e.clone(), false));
    }
    group.add(&row);
    me.rows.borrow_mut().push(row);
}

/// The editor for an event (`new`: not saved yet).
fn edit_event(me: &Rc<App>, e: Event, new: bool) {
    let before = (!new).then(|| e.clone());
    let start = e.start().unwrap_or(When::Day(selected(&me.month)));
    let end = e.end().unwrap_or(start);
    let all_day = matches!(start, When::Day(_));
    let hm = |w: When, default: &str| match w {
        When::At(t) => t.format("%H:%M").to_string(),
        When::Day(_) => default.to_string(),
    };

    let page = adw::PreferencesPage::new();
    let what = adw::PreferencesGroup::new();
    let title = adw::EntryRow::builder().title("Title").text(e.get("title")).build();
    let libs = me.libraries.borrow().clone();
    let library = adw::ComboRow::builder().title("Calendar").model(&gtk::StringList::new(&libs.iter().map(|l| if l == ME { "Mine (me)" } else { l.as_str() }).collect::<Vec<_>>())).build();
    library.set_selected(libs.iter().position(|l| *l == e.ns).unwrap_or(0) as u32);
    library.set_sensitive(new);
    what.add(&title);
    what.add(&library);
    page.add(&what);

    let when = adw::PreferencesGroup::builder().title("When").build();
    let date = adw::EntryRow::builder().title("Date (YYYY-MM-DD)").text(start.date().format("%Y-%m-%d").to_string()).build();
    let whole = adw::SwitchRow::builder().title("All day").active(all_day).build();
    let from = adw::EntryRow::builder().title("From (HH:MM)").text(hm(start, "12:00")).build();
    let to = adw::EntryRow::builder().title("Until (HH:MM)").text(hm(end, "13:00")).build();
    from.set_visible(!all_day);
    to.set_visible(!all_day);
    {
        let (from, to) = (from.clone(), to.clone());
        whole.connect_active_notify(move |w| {
            from.set_visible(!w.is_active());
            to.set_visible(!w.is_active());
        });
    }
    let repeat = adw::ComboRow::builder().title("Repeats").model(&gtk::StringList::new(&REPEATS.iter().map(|(n, _)| *n).collect::<Vec<_>>())).build();
    repeat.set_selected(e.repeat_index() as u32);
    when.add(&date);
    when.add(&whole);
    when.add(&from);
    when.add(&to);
    when.add(&repeat);
    page.add(&when);

    let where_ = adw::PreferencesGroup::builder().title("Where and what").build();
    let location = adw::EntryRow::builder().title("Location").text(e.get("location")).build();
    let note = adw::EntryRow::builder().title("Note").text(e.get("note")).build();
    where_.add(&location);
    where_.add(&note);
    page.add(&where_);

    // Attendees: contacts from your own address book and the shown libraries.
    let who = adw::PreferencesGroup::builder().title("Who").description("Attendees are your contacts; their cards list the events they're at.").build();
    page.add(&who);
    let picks: Rc<RefCell<Vec<(Person, gtk::CheckButton)>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let (who, picks, chosen) = (who.clone(), picks.clone(), e.attendees());
        bg(&me.cal, move |c| c.people(&libs), move |people: Vec<Person>| {
            for p in people {
                let check = gtk::CheckButton::builder().active(chosen.iter().any(|(id, _)| *id == p.id)).valign(gtk::Align::Center).build();
                let row = adw::ActionRow::builder().title(glib::markup_escape_text(&p.name)).subtitle(if p.ns == ME { String::new() } else { p.ns.clone() }).activatable_widget(&check).build();
                row.add_prefix(&adw::Avatar::new(28, Some(&p.name), true));
                row.add_suffix(&check);
                who.add(&row);
                picks.borrow_mut().push((p, check));
            }
        });
    }

    let save = gtk::Button::builder().label("Save").build();
    save.add_css_class("suggested-action");
    let delete = gtk::Button::from_icon_name("user-trash-symbolic");
    delete.set_visible(!new);
    let share = gtk::Button::builder().label("Share…").visible(!new).tooltip_text("A link to this event alone: whoever opens it sees it, and sees it change").build();
    let header = adw::HeaderBar::new();
    header.pack_end(&save);
    header.pack_start(&delete);
    header.pack_start(&share);
    {
        let (me2, e2) = (me.clone(), e.clone());
        share.connect_clicked(move |_| {
            let (me3, e3) = (me2.clone(), e2.clone());
            bg(&me2.cal, move |c| c.share_event(&e3), move |r| show_link(&me3, "Share this event", "Whoever opens this link sees this event, and sees it when it changes. Nothing else of the calendar.", r));
        });
    }
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&page));
    let dialog = adw::Window::builder().transient_for(&me.window).modal(true).title(if new { "New event" } else { "Event" }).default_width(520).default_height(640).content(&view).build();

    let before_for_delete = before.clone();
    {
        let (me2, dialog2) = (me.clone(), dialog.clone());
        save.connect_clicked(move |_| {
            let mut ev = e.clone();
            if new {
                ev.ns = libs_at(&me2, library.selected());
            }
            let form = Form {
                title: title.text().into(),
                date: date.text().into(),
                all_day: whole.is_active(),
                from: from.text().into(),
                until: to.text().into(),
                repeat: repeat.selected() as usize,
                location: location.text().into(),
                note: note.text().into(),
                attendees: picks.borrow().iter().filter(|(_, c)| c.is_active()).map(|(p, _)| (p.id.clone(), p.name.clone())).collect(),
            };
            ev.apply_form(&form, start.date());
            let (me3, dialog3, before) = (me2.clone(), dialog2.clone(), before.clone());
            bg(&me2.cal, move |c| c.save(&ev, before.as_ref()), move |r| {
                match r {
                    Ok(saved) => {
                        if let Some(s) = saved.start() {
                            select(&me3.month, s.date());
                        }
                        dialog3.close();
                    }
                    Err(err) => dialog3.set_title(Some(&format!("Not saved: {err}"))),
                }
                reload(&me3);
            });
        });
    }
    {
        let (me2, dialog2, e2) = (me.clone(), dialog.clone(), before_for_delete);
        delete.connect_clicked(move |_| {
            let Some(e) = e2.clone() else { return };
            let (me3, dialog3) = (me2.clone(), dialog2.clone());
            bg(&me2.cal, move |c| c.delete(&e), move |_| {
                dialog3.close();
                reload(&me3);
            });
        });
    }
    dialog.present();
}

/// Sharing a calendar: with a colleague who writes to it too, or for reading,
/// all of it or only when it's busy.
fn share_calendar_dialog(me: &Rc<App>) {
    let libs: Vec<String> = me.libraries.borrow().iter().filter(|l| *l != ME).cloned().collect();
    if libs.is_empty() {
        show_link(me, "Share a calendar", "", Err("Your own calendar (me) is yours alone. Make a calendar to share first.".into()));
        return;
    }
    let pick = gtk::DropDown::from_strings(&libs.iter().map(String::as_str).collect::<Vec<_>>());
    let dialog = adw::MessageDialog::builder()
        .heading("Share a calendar")
        .body("A member reads and writes it (a colleague). A reader sees all of it. Free/busy shows only when it's busy: no titles, places, notes or people.")
        .extra_child(&pick)
        .transient_for(&me.window)
        .close_response("cancel")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("busy", "Free/busy"), ("read", "Reader"), ("member", "Member")]);
    dialog.set_response_appearance("member", adw::ResponseAppearance::Suggested);
    let me2 = me.clone();
    dialog.connect_response(None, move |_, r| {
        let Some(ns) = libs.get(pick.selected() as usize).cloned() else { return };
        let me3 = me2.clone();
        match r {
            "member" => bg(&me2.cal, move |c| c.invite_link(&ns), move |l| show_link(&me3, "Invite a member", "Whoever opens this invitation first joins the calendar and may write to it. It works once, for a week.", l)),
            "read" => bg(&me2.cal, move |c| c.share_calendar(&ns, false), move |l| show_link(&me3, "Share for reading", "Whoever has this link reads the whole calendar.", l)),
            "busy" => bg(&me2.cal, move |c| c.share_calendar(&ns, true), move |l| show_link(&me3, "Share free/busy", "Whoever has this link sees when the calendar is busy, and nothing else.", l)),
            _ => {}
        }
    });
    dialog.present();
}

/// A link to give someone, to copy.
fn show_link(me: &Rc<App>, heading: &str, body: &str, link: Result<String, String>) {
    let (body, link) = match link {
        Ok(l) => (body.to_string(), Some(l)),
        Err(e) => (e, None),
    };
    let dialog = adw::MessageDialog::builder().heading(heading).body(body).transient_for(&me.window).close_response("close").build();
    if let Some(link) = &link {
        let label = gtk::Label::builder().label(link).selectable(true).wrap(true).wrap_mode(gtk::pango::WrapMode::Char).css_classes(["monospace", "caption"]).build();
        dialog.set_extra_child(Some(&label));
        dialog.add_response("copy", "Copy link");
    }
    dialog.add_response("close", "Close");
    dialog.connect_response(None, move |d, r| {
        if r == "copy" {
            if let Some(l) = &link {
                d.clipboard().set_text(l);
            }
        }
    });
    dialog.present();
}

fn libs_at(me: &Rc<App>, i: u32) -> String {
    me.libraries.borrow().get(i as usize).cloned().unwrap_or_else(|| ME.into())
}

/// RETIFS_CALENDAR_SNAPSHOT=<file.png>: the window drawn to a file every 2 s.
fn snapshot_on_request(window: &adw::ApplicationWindow) {
    let Ok(path) = std::env::var("RETIFS_CALENDAR_SNAPSHOT") else { return };
    let window = window.clone();
    glib::timeout_add_seconds_local(2, move || {
        let paintable = gtk::WidgetPaintable::new(Some(&window));
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
        if let (Some(node), Some(renderer)) = (snapshot.to_node(), window.renderer()) {
            let _ = renderer.render_texture(&node, None).save_to_png(&path);
        }
        glib::ControlFlow::Continue
    });
}
