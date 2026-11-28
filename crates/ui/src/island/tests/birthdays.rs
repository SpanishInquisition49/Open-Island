//! The card and the confetti of a birthday in the calendars.

use super::*;

const H: i64 = 3_600_000;

/// Ten in the morning (UTC) of the day `T0` is in.
const MORNING: i64 = T0 - T0 % (24 * H) + 10 * H;

fn entry(title: &str, all_day: bool) -> Event {
    let midnight = MORNING - 10 * H;
    let (start_ms, end_ms) = if all_day { (midnight, midnight + 24 * H) } else { (MORNING + H, MORNING + 2 * H) };
    Event { id: title.into(), title: title.into(), start_ms, end_ms, all_day, ..Default::default() }
}

fn with(events: Vec<Event>, cfg: Config) -> Island {
    let mut i = Island::new(Config { meeting_alerts: false, end_alerts: false, evening_recap_min: 0, morning_recap_min: 0, ..cfg });
    i.data.events = events;
    i.tick(MORNING, 8.0);
    run(&mut i, 1000);
    i
}

#[test]
fn a_birthday_today_brings_a_card_with_confetti_once() {
    let mut i = with(vec![entry("Compleanno di Ada", true), entry("Ada's birthday", true), entry("🎂 Alan", true), entry("Cena per il compleanno", false)], Config::default());
    let card = i.notif.clone().expect("a card");
    assert_eq!((card.summary.as_str(), card.body.as_str()), ("2 birthdays today", "Ada\nAlan"));
    let born = i.fx.confetti.as_ref().and_then(|c| c.born).expect("the confetti has burst");
    let fr = i.tick(born + 200, 8.0);
    assert!(fr.animating && fr.surface.1 > 200.0, "{:?}", fr.surface);
    i.tick(born + island_core::birthday::CONFETTI_MS + 10, 8.0);
    assert!(i.fx.confetti.is_none());
    // not again today, also after a restart that remembers the day
    let day = i.birthday_day();
    i.notif = None;
    i.tick(MORNING + 2 * H, 8.0);
    assert!(i.fx.confetti.is_none() && i.notif.is_none());
    let mut again = Island::new(Config::default());
    again.restore_birthday_day(day);
    again.data.events = vec![entry("Compleanno di Ada", true)];
    again.tick(MORNING + 3 * H, 8.0);
    assert!(again.fx.confetti.is_none());
}

#[test]
fn no_birthday_no_card_and_none_when_switched_off_or_before_eight() {
    for events in [vec![], vec![entry("Algorithms", true)], vec![entry("Compleanno di Ada", false)]] {
        let i = with(events, Config::default());
        assert!(i.fx.confetti.is_none());
    }
    assert!(with(vec![entry("Compleanno di Ada", true)], Config { birthday_cards: false, ..Config::default() }).fx.confetti.is_none());
    let mut early = Island::new(Config { evening_recap_min: 0, morning_recap_min: 0, ..Config::default() });
    early.data.events = vec![entry("Compleanno di Ada", true)];
    early.tick(MORNING - 3 * H, 8.0);
    assert!(early.notif.is_none());
    early.tick(MORNING, 8.0);
    assert!(early.fx.confetti.is_some(), "at the first moment it may");
}

#[test]
fn it_waits_while_the_island_is_quiet_and_can_be_asked_for_to_look_at() {
    let mut i = Island::new(Config { evening_recap_min: 0, morning_recap_min: 0, ..Config::default() });
    i.data.dnd = true;
    i.data.events = vec![entry("Compleanno di Ada", true)];
    i.tick(MORNING, 8.0);
    assert!(i.fx.confetti.is_none());
    i.data.dnd = false;
    i.tick(MORNING + H, 8.0);
    assert!(i.fx.confetti.is_some());
    let mut look = Island::new(Config { birthday_cards: false, ..Config::default() });
    look.push(T0, Input::PreviewBirthday("Grace".into()));
    run(&mut look, 800);
    assert!(look.notif.as_ref().is_some_and(|n| n.body == "Grace"));
    assert!(look.fx.confetti.is_some());
    assert_eq!(look.birthday_day(), i64::MIN, "the real one still comes");
}
