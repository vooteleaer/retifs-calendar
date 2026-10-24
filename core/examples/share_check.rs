//! Shares a calendar three ways and one event, printing the links:
//! `cargo run -p retifs-calendar-core --example share_check -- CALENDAR`.
use retifs_calendar_core::Calendar;

fn main() {
    let ns = std::env::args().nth(1).unwrap_or_else(|| "cafa".into());
    let cal = Calendar::new();
    println!("member: {:?}", cal.invite_link(&ns));
    println!("reader: {:?}", cal.share_calendar(&ns, false));
    println!("busy:   {:?}", cal.share_calendar(&ns, true));
    let today = chrono::Local::now().date_naive();
    let list = cal.occurrences(&[ns.clone()], today - chrono::Duration::days(30), today + chrono::Duration::days(60), false).unwrap_or_default();
    if let Some(o) = list.iter().find(|o| o.event.get("title").contains("kliend")) {
        println!("event:  {:?}", cal.share_event(&o.event));
    }
    println!("calendars now: {:?}", cal.libraries());
}
