//! A small CalDAV client (RFC 4791): enough to read every calendar of an account (iCloud, Nextcloud, Fastmail,
//! Radicale...). It discovers the principal, the calendar home and the calendars (name, colour), then asks each
//! for the events of a time window. Read-only; the credentials only ever go to the server they were given for.

use base64::Engine;
use roxmltree::{Document, Node};

pub const DAV: &str = "DAV:";
pub const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
pub const APPLE_ICAL: &str = "http://apple.com/ns/ical/";

/// A calendar found in an account.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub href: String,
    pub name: String,
    pub color: Option<(u8, u8, u8)>,
}

pub fn basic_auth(user: &str, password: &str) -> String {
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}")))
}

pub const PRINCIPAL_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;
pub const HOME_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><c:calendar-home-set/></d:prop></d:propfind>"#;
pub const LIST_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/"><d:prop><d:displayname/><d:resourcetype/><a:calendar-color/><c:supported-calendar-component-set/></d:prop></d:propfind>"#;

/// The REPORT body asking for the events overlapping [from, to) (UTC, "20261118T000000Z").
pub fn query_body(from: &str, to: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{from}" end="{to}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#
    )
}

fn named<'a, 'i>(n: Node<'a, 'i>, ns: &str, local: &str) -> bool {
    n.is_element() && n.tag_name().name() == local && n.tag_name().namespace() == Some(ns)
}

fn first<'a, 'i>(root: Node<'a, 'i>, ns: &str, local: &str) -> Option<Node<'a, 'i>> {
    root.descendants().find(|n| named(*n, ns, local))
}

fn text_of(n: Node) -> String {
    n.descendants().filter(|d| d.is_text()).map(|d| d.text().unwrap_or("")).collect::<String>().trim().to_string()
}

/// The `href` inside `<current-user-principal>`.
pub fn parse_principal(xml: &str) -> Option<String> {
    let doc = Document::parse(xml).ok()?;
    let p = first(doc.root(), DAV, "current-user-principal")?;
    first(p, DAV, "href").map(text_of).filter(|h| !h.is_empty())
}

/// The `href` inside `<calendar-home-set>`.
pub fn parse_home(xml: &str) -> Option<String> {
    let doc = Document::parse(xml).ok()?;
    let p = first(doc.root(), CALDAV, "calendar-home-set")?;
    first(p, DAV, "href").map(text_of).filter(|h| !h.is_empty())
}

/// "#FF2968FF" / "#FF2968" -> (255, 41, 104).
pub fn parse_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();
    (s.len() >= 7).then(|| island_core::color::parse_hex(&s[..7])).flatten()
}

/// The event calendars of a home-set listing (not the inbox, outbox, reminders lists or address books).
pub fn parse_calendars(xml: &str) -> Vec<Found> {
    parse_calendars_for(xml, "VEVENT")
}

/// The calendars of a listing that hold `comp` entries (`VEVENT` for events, `VTODO` for to-dos); one that does not say
/// what it holds is taken for both.
pub fn parse_calendars_for(xml: &str, comp: &str) -> Vec<Found> {
    let Ok(doc) = Document::parse(xml) else { return vec![] };
    let mut out = vec![];
    for resp in doc.descendants().filter(|n| named(*n, DAV, "response")) {
        let Some(href) = first(resp, DAV, "href").map(text_of) else { continue };
        let Some(rt) = first(resp, DAV, "resourcetype") else { continue };
        if !rt.descendants().any(|n| named(n, CALDAV, "calendar")) {
            continue;
        }
        // a calendar that holds only reminders lists VTODO and no VEVENT
        if let Some(set) = first(resp, CALDAV, "supported-calendar-component-set") {
            let comps: Vec<_> = set.descendants().filter(|n| named(*n, CALDAV, "comp")).collect();
            if !comps.is_empty() && !comps.iter().any(|c| c.attribute("name") == Some(comp)) {
                continue;
            }
        }
        let name = first(resp, DAV, "displayname").map(text_of).filter(|n| !n.is_empty()).unwrap_or_else(|| href.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string());
        let color = first(resp, APPLE_ICAL, "calendar-color").map(text_of).and_then(|c| parse_color(&c));
        out.push(Found { href, name, color });
    }
    out
}

/// The REPORT body asking for every to-do of a calendar.
pub fn todo_query_body() -> String {
    r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VTODO"/></c:comp-filter></c:filter></c:calendar-query>"#.to_string()
}

/// The to-dos of a calendar-query answer: (the `href` of the entry, its iCalendar document).
pub fn parse_todo_report(xml: &str) -> Vec<(String, String)> {
    let Ok(doc) = Document::parse(xml) else { return vec![] };
    doc.descendants()
        .filter(|n| named(*n, DAV, "response"))
        .filter_map(|resp| {
            let data = first(resp, CALDAV, "calendar-data").map(|n| n.text().unwrap_or("").to_string()).filter(|t| t.contains("BEGIN:VTODO"))?;
            Some((first(resp, DAV, "href").map(text_of).unwrap_or_default(), data))
        })
        .collect()
}

/// (address, iCalendar document) of each event resource in a calendar-query answer.
pub fn parse_event_report(xml: &str) -> Vec<(String, String)> {
    let Ok(doc) = Document::parse(xml) else { return vec![] };
    doc.descendants()
        .filter(|n| named(*n, DAV, "response"))
        .filter_map(|resp| {
            let data = first(resp, CALDAV, "calendar-data").map(|n| n.text().unwrap_or("").to_string()).filter(|t| t.contains("BEGIN:VEVENT"))?;
            Some((first(resp, DAV, "href").map(text_of).unwrap_or_default(), data))
        })
        .collect()
}

/// The iCalendar documents (one per event) inside a calendar-query answer.
pub fn parse_report(xml: &str) -> Vec<String> {
    let Ok(doc) = Document::parse(xml) else { return vec![] };
    doc.descendants().filter(|n| named(*n, CALDAV, "calendar-data")).map(|n| n.text().unwrap_or("").to_string()).filter(|t| t.contains("BEGIN:VEVENT")).collect()
}

/// Joins a server-relative or absolute `href` to the server that answered.
pub fn resolve(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    let scheme_end = base.find("://").map_or(0, |i| i + 3);
    let host_end = base[scheme_end..].find('/').map_or(base.len(), |i| scheme_end + i);
    if href.starts_with('/') {
        format!("{}{}", &base[..host_end], href)
    } else {
        format!("{}/{}", base.trim_end_matches('/'), href)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRINCIPAL: &str = r#"<?xml version="1.0"?><multistatus xmlns="DAV:"><response><href>/</href><propstat><prop><current-user-principal><href>/1234567/principal/</href></current-user-principal></prop><status>HTTP/1.1 200 OK</status></propstat></response></multistatus>"#;
    const HOME: &str = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/1234567/principal/</d:href><d:propstat><d:prop><c:calendar-home-set><d:href>https://p01-caldav.icloud.com:443/1234567/calendars/</d:href></c:calendar-home-set></d:prop></d:propstat></d:response></d:multistatus>"#;
    const LIST: &str = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/">
      <d:response><d:href>/1234567/calendars/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
      <d:response><d:href>/1234567/calendars/home/</d:href><d:propstat><d:prop><d:displayname>Personal</d:displayname><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><a:calendar-color>#FF2968FF</a:calendar-color>
        <c:supported-calendar-component-set><c:comp name="VEVENT"/><c:comp name="VTODO"/></c:supported-calendar-component-set></d:prop></d:propstat></d:response>
      <d:response><d:href>/1234567/calendars/reminders/</d:href><d:propstat><d:prop><d:displayname>Reminders</d:displayname><d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
        <c:supported-calendar-component-set><c:comp name="VTODO"/></c:supported-calendar-component-set></d:prop></d:propstat></d:response>
      <d:response><d:href>/1234567/calendars/inbox/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:schedule-inbox/></d:resourcetype></d:prop></d:propstat></d:response>
      <d:response><d:href>/1234567/calendars/work/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype></d:prop></d:propstat></d:response></d:multistatus>"#;
    const REPORT: &str = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
      <d:response><d:href>/x/1.ics</d:href><d:propstat><d:prop><d:getetag>"a"</d:getetag><c:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
UID:one
SUMMARY:Lezione
DTSTART:20261118T150000Z
DTEND:20261118T163000Z
END:VEVENT
END:VCALENDAR
</c:calendar-data></d:prop></d:propstat></d:response>
      <d:response><d:href>/x/2.ics</d:href><d:propstat><d:prop><c:calendar-data>BEGIN:VCALENDAR
BEGIN:VTODO
UID:t
SUMMARY:Do it
END:VTODO
END:VCALENDAR</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>"#;

    #[test]
    fn discovers_the_principal_and_the_home() {
        assert_eq!(parse_principal(PRINCIPAL).as_deref(), Some("/1234567/principal/"));
        assert_eq!(parse_home(HOME).as_deref(), Some("https://p01-caldav.icloud.com:443/1234567/calendars/"));
        assert_eq!(parse_principal("<nope/>"), None);
        assert_eq!(parse_home("not xml"), None);
    }

    #[test]
    fn lists_only_event_calendars_with_their_name_and_colour() {
        let l = parse_calendars(LIST);
        assert_eq!(l.len(), 2, "{l:?}");
        assert_eq!(l[0], Found { href: "/1234567/calendars/home/".into(), name: "Personal".into(), color: Some((0xff, 0x29, 0x68)) });
        assert_eq!(l[1].name, "work", "the folder name stands in for a missing display name");
        assert_eq!(l[1].color, None);
        assert!(parse_calendars("garbage").is_empty());
    }

    #[test]
    fn lists_the_calendars_that_hold_to_dos() {
        let l = parse_calendars_for(LIST, "VTODO");
        assert_eq!(l.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Personal", "Reminders", "work"], "a list that does not say what it holds counts too");
        assert_eq!(parse_calendars(LIST).len(), 2, "events: unchanged");
    }

    #[test]
    fn extracts_the_to_dos_of_a_report_with_where_they_live() {
        let t = parse_todo_report(REPORT);
        assert_eq!(t.len(), 1, "the event is not a to-do");
        assert_eq!(t[0].0, "/x/2.ics");
        assert_eq!(island_core::task::from_vtodo(&t[0].1)[0].0.uid, "t");
        assert!(todo_query_body().contains(r#"comp-filter name="VTODO""#));
    }

    #[test]
    fn extracts_the_events_of_a_report() {
        let r = parse_report(REPORT);
        assert_eq!(r.len(), 1, "the to-do is not an event");
        let evs = island_core::ics::parse(&r[0], 0, 0, i64::MAX);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].title, "Lezione");
        assert!(parse_report("<nope/>").is_empty());
    }

    #[test]
    fn urls_and_auth_and_colours() {
        assert_eq!(resolve("https://caldav.icloud.com/", "/1234/principal/"), "https://caldav.icloud.com/1234/principal/");
        assert_eq!(resolve("https://caldav.icloud.com/x/", "https://p01.icloud.com:443/a/"), "https://p01.icloud.com:443/a/");
        assert_eq!(resolve("https://h.org/dav", "calendars/"), "https://h.org/dav/calendars/");
        assert_eq!(basic_auth("me@icloud.com", "abcd-efgh"), "Basic bWVAaWNsb3VkLmNvbTphYmNkLWVmZ2g=");
        assert_eq!(parse_color("#FF9500FF"), Some((255, 149, 0)));
        assert_eq!(parse_color("#12"), None);
        assert!(query_body("20261118T000000Z", "20270118T000000Z").contains(r#"start="20261118T000000Z""#));
    }
}
