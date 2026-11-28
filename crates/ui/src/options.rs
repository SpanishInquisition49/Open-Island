//! The options page: which options there are, on which tab, and how they map onto the island's configuration.
//! The names follow the settings of the Noctalia island plugin, so a setting there is a setting here.

use crate::island::Config;

/// The tabs, by what the options are for: how the island looks and behaves (Island), what it shows you (Alerts), when it keeps
/// quiet (Quiet), your day (Day), where you are and what it asks the network (Places), the player (Music), notification rules.
pub const TABS: [&str; 7] = ["Island", "Alerts", "Quiet", "Day", "Places", "Music", "Rules"];

/// The tab that lists the notification rules.
pub const RULES_TAB: usize = 6;

/// (key, label) of the switches on a tab.
pub fn toggles(tab: usize) -> &'static [(&'static str, &'static str)] {
    match tab {
        0 => &[
            ("clock_in_pill", "Clock in the small pill"),
            ("status_icons", "Status icons"),
            ("hide_on_fullscreen", "Hide under fullscreen"),
            ("hide_when_covered", "Hide under windows"),
            ("clipboard_history", "Clipboard history"),
            ("timer_sound", "Timer sound"),
            ("watch_downloads", "Show downloads"),
            ("progress_drop", "Progress drop"),
        ],
        1 => &[
            ("show_notifications", "Show notifications"),
            ("show_osd", "Volume & brightness"),
            ("battery_alerts", "Battery alerts"),
            ("meeting_alerts", "Meeting heads-up"),
            ("rain_alerts", "Rain alerts"),
            ("culprit_alerts", "Heavy program alerts"),
            ("ocr_screenshots", "Text from screenshots"),
            ("keyboard_alerts", "Keyboard changes"),
        ],
        2 => &[("quiet_in_meetings", "Quiet in meetings"), ("quiet_on_mic", "Quiet on a call"), ("quiet_on_screen_share", "Quiet when sharing"), ("away_recap", "Recap when back")],
        3 => &[("travel_times", "Travel times"), ("evening_weekends", "Cards on weekends"), ("link_cards", "Copied link cards"), ("end_alerts", "Event ending")],
        4 => &[("weather", "Weather"), ("follow_position", "Follow my position"), ("wifi_position", "Precise position from Wi-Fi"), ("currency_rates", "Currency rates"), ("receive_files", "Receive files")],
        5 => &[("media_auto_expand", "Open on a new track"), ("lyrics", "Synced lyrics"), ("lyrics_in_pill", "Lyrics in the pill"), ("art_tint", "Tint from the cover"), ("auto_audio_output", "Audio to headphones")],
        _ => &[],
    }
}

/// What an option controls, in one line: shown under the options when the pointer rests on one.
pub fn describe(key: &str) -> &'static str {
    match key {
        "clock_in_pill" => "Show the time inside the small pill when nothing else is on",
        "status_icons" => "Icons in the pill for the microphone, camera, screen share and quiet mode",
        "clipboard_history" => "Keep a list of what you copy (pinned entries survive restarts)",
        "hide_on_fullscreen" => "Slide the island away while a window is fullscreen",
        "hide_when_covered" => "Slide away from a window over it; the pointer at the top edge calls it back",
        "ocr_screenshots" => "A screenshot card offers to copy the text in the picture",
        "watch_downloads" => "Show browser downloads as they come (reads the download folder)",
        "receive_files" => "LocalSend: nearby devices can send files, you accept each one (Linux)",
        "keyboard_alerts" => "Say when the keyboard layout or Caps Lock changes (Linux)",
        "progress_drop" => "A drop beside the pill: it fills with a job, drains with a countdown",
        "link_cards" => "Offer to open a copied link, address or e-mail (needs clipboard history)",
        "currency_rates" => "Fetch the ECB daily rates (20 usd in eur in the command bar)",
        "wifi_position" => "Send nearby Wi-Fi addresses to beacondb.net for a position within metres",
        "follow_position" => "Weather, trips and the map use where you are; off: the settings coordinates",
        "weather" => "Forecast, rain alerts and the weather page (needs a location)",
        "timer_sound" => "Play a sound when a timer ends",
        "auto_audio_output" => "Switch the sound to headphones when they connect, back when they go",
        "show_notifications" => "Show incoming notifications as cards on the island",
        "show_osd" => "Show a bar when the volume or the brightness changes",
        "battery_alerts" => "Cards for plugging in, unplugging and a low battery",
        "meeting_alerts" => "Remind of calendar events before they start (card and pill)",
        "end_alerts" => "Five minutes before an event ends: what comes next, and where",
        "rain_alerts" => "Warn when rain is expected soon",
        "quiet_in_meetings" => "Hold notifications during a calendar event",
        "quiet_on_mic" => "Hold notifications while the microphone is in use",
        "quiet_on_screen_share" => "Hold notifications while you share the screen",
        "away_recap" => "A card with what came in while you were away, when you are back",
        "evening_recap_time" => "At this time, a card with tomorrow: events, reminders, to-dos (off: never)",
        "evening_weekends" => "Show the morning and evening cards on Saturday and Sunday too",
        "morning_recap_time" => "From this time, on first use: a card with what is left of today (off: never)",
        "culprit_alerts" => "Name the program that hogs the CPU or the memory",
        "media_auto_expand" => "Open the island for a moment when a new track starts",
        "lyrics" => "Show synced lyrics on the music page",
        "lyrics_in_pill" => "Show the current lyric line in the small pill",
        "art_tint" => "Tint the island with the colours of the album cover",
        "notification_seconds" => "How long a notification card stays on screen",
        "osd_seconds" => "How long the volume and brightness bar stays",
        "low_battery_percent" => "The battery level that gives the low battery warning",
        "meeting_lead_minutes" => "How long before an event its card appears",
        "meeting_pill_minutes" => "How long before an event the pill on the side appears",
        "dock_icons" => "How many page icons the bottom bar shows (the rest: All pages)",
        "mini_pills" => "How many small pills may sit beside the main one (timer, focus, activities)",
        "travel_margin_minutes" => "How much earlier than needed to be told to leave for an event",
        "distraction_minutes" => "Told after this long on apps marked on the Activity page, in study hours",
        "countdown_days" => "The clock pill counts down to a deadline this many days away (0: off)",
        "commute_minutes" => "Door to door to your lectures: cards say when to leave (0: off)",
        "deep_work_minutes" => "Hold notifications after this long on work apps in a row (0: off)",
        "break_minutes" => "A card after this long on one app without a break (0: off)",
        "travel_times" => "Time the way to events with a place (sends the place to map services)",
        "timer_presets" => "The quick timers on the timer page, such as 5m, 10m, 1h30m",
        "map_tile_url" => "Tile URL with {z} {x} {y} and {key} for the API key (empty: OpenStreetMap)",
        "screen" => "The screen the island sits on (Wayland): a name like eDP-1, or empty",
        _ => "",
    }
}

/// Height of the line that explains the option under the pointer.
pub const HELP_H: f64 = 30.0;

/// A number with its step and range: (key, label, unit, step, min, max).
pub type Stepper = (&'static str, &'static str, &'static str, f64, f64, f64);

pub fn steppers(tab: usize) -> &'static [Stepper] {
    match tab {
        1 => &[
            ("notification_seconds", "Card time", "s", 1.0, 2.0, 30.0),
            ("osd_seconds", "Bar time", "s", 0.1, 0.5, 5.0),
            ("low_battery_percent", "Low battery", "%", 1.0, 5.0, 50.0),
            ("meeting_lead_minutes", "Event card", "min", 1.0, 1.0, 60.0),
            ("meeting_pill_minutes", "Event pill", "min", 1.0, 1.0, 120.0),
        ],
        0 => &[("mini_pills", "Side pills", "", 1.0, 1.0, 4.0), ("dock_icons", "Dock icons", "", 1.0, 4.0, 12.0)],
        2 => &[("deep_work_minutes", "Auto focus", "min", 5.0, 0.0, 120.0), ("break_minutes", "Break nudge", "min", 15.0, 0.0, 240.0), ("distraction_minutes", "Distraction", "min", 5.0, 0.0, 120.0)],
        3 => &[
            ("morning_recap_time", "Morning", "time", 15.0, 0.0, 1425.0),
            ("evening_recap_time", "Evening", "time", 15.0, 0.0, 1425.0),
            ("commute_minutes", "Commute", "min", 5.0, 0.0, 240.0),
            ("travel_margin_minutes", "Leave early", "min", 1.0, 0.0, 30.0),
            ("countdown_days", "Countdown", "d", 1.0, 0.0, 365.0),
        ],
        _ => &[],
    }
}

/// The text fields a tab has, by option key (the page knows how to draw and edit each): the timer presets and the screen
/// under Island, the map tiles under Places.
pub fn text_keys(tab: usize) -> &'static [&'static str] {
    match tab {
        0 => &["timer_presets", "screen"],
        4 => &["map_tile_url"],
        _ => &[],
    }
}

/// How many rows of text fields a tab has.
pub fn text_rows(tab: usize) -> usize {
    text_keys(tab).len()
}

/// A line under the tab's title that says what the tab is for (shown when the tab has room for it).
pub fn tab_blurb(tab: usize) -> &'static str {
    match tab {
        0 => "How the island looks and behaves",
        1 => "What it shows you, and for how long",
        2 => "When it keeps its notifications to itself",
        3 => "Your day: cards at set times, the way to lectures",
        4 => "Where you are, and what it asks the network",
        5 => "The player",
        _ => "",
    }
}

pub fn get_bool(c: &Config, key: &str) -> bool {
    match key {
        "show_notifications" => c.show_notifications,
        "show_osd" => c.show_osd,
        "media_auto_expand" => c.media_auto_expand,
        "timer_sound" => c.timer_sound,
        "auto_audio_output" => c.auto_audio_output,
        "follow_position" => c.follow_position,
        "wifi_position" => c.wifi_position,
        "currency_rates" => c.currency_rates,
        "watch_downloads" => c.watch_downloads,
        "ocr_screenshots" => c.ocr_screenshots,
        "receive_files" => c.receive_files,
        "keyboard_alerts" => c.keyboard_alerts,
        "battery_alerts" => c.battery_alerts,
        "weather" => c.weather,
        "rain_alerts" => c.rain_alerts,
        "meeting_alerts" => c.meeting_alerts,
        "end_alerts" => c.end_alerts,
        "lyrics" => c.lyrics,
        "lyrics_in_pill" => c.lyrics_in_pill,
        "art_tint" => c.art_tint,
        "status_icons" => c.status_icons,
        "clipboard_history" => c.clipboard_history,
        "progress_drop" => c.progress_drop,
        "link_cards" => c.link_cards,
        "clock_in_pill" => c.clock_in_pill,
        "hide_on_fullscreen" => c.hide_on_fullscreen,
        "hide_when_covered" => c.hide_when_covered,
        "quiet_in_meetings" => c.quiet_in_meetings,
        "quiet_on_mic" => c.quiet_on_mic,
        "away_recap" => c.away_recap,
        "evening_weekends" => c.evening_weekends,
        "culprit_alerts" => c.culprit_alerts,
        "travel_times" => c.travel_times,
        "quiet_on_screen_share" => c.quiet_on_screen_share,
        _ => false,
    }
}

pub fn set_bool(c: &mut Config, key: &str, on: bool) {
    match key {
        "show_notifications" => c.show_notifications = on,
        "show_osd" => c.show_osd = on,
        "media_auto_expand" => c.media_auto_expand = on,
        "timer_sound" => c.timer_sound = on,
        "auto_audio_output" => c.auto_audio_output = on,
        "follow_position" => c.follow_position = on,
        "wifi_position" => c.wifi_position = on,
        "currency_rates" => c.currency_rates = on,
        "watch_downloads" => c.watch_downloads = on,
        "ocr_screenshots" => c.ocr_screenshots = on,
        "receive_files" => c.receive_files = on,
        "keyboard_alerts" => c.keyboard_alerts = on,
        "battery_alerts" => c.battery_alerts = on,
        "weather" => c.weather = on,
        "rain_alerts" => c.rain_alerts = on,
        "meeting_alerts" => c.meeting_alerts = on,
        "end_alerts" => c.end_alerts = on,
        "lyrics" => c.lyrics = on,
        "lyrics_in_pill" => c.lyrics_in_pill = on,
        "art_tint" => c.art_tint = on,
        "status_icons" => c.status_icons = on,
        "clipboard_history" => c.clipboard_history = on,
        "progress_drop" => c.progress_drop = on,
        "link_cards" => c.link_cards = on,
        "clock_in_pill" => c.clock_in_pill = on,
        "hide_on_fullscreen" => c.hide_on_fullscreen = on,
        "hide_when_covered" => c.hide_when_covered = on,
        "quiet_in_meetings" => c.quiet_in_meetings = on,
        "quiet_on_mic" => c.quiet_on_mic = on,
        "away_recap" => c.away_recap = on,
        "evening_weekends" => c.evening_weekends = on,
        "culprit_alerts" => c.culprit_alerts = on,
        "travel_times" => c.travel_times = on,
        "quiet_on_screen_share" => c.quiet_on_screen_share = on,
        _ => {}
    }
}

pub fn get_num(c: &Config, key: &str) -> f64 {
    match key {
        "notification_seconds" => c.notification_ms as f64 / 1000.0,
        "osd_seconds" => c.osd_ms as f64 / 1000.0,
        "low_battery_percent" => c.low_battery as f64,
        "meeting_lead_minutes" => c.meeting_lead_ms as f64 / 60_000.0,
        "meeting_pill_minutes" => c.meeting_pill_ms as f64 / 60_000.0,
        "travel_margin_minutes" => c.travel_margin_ms as f64 / 60_000.0,
        "mini_pills" => c.mini_pills as f64,
        "dock_icons" => c.dock_icons as f64,
        "deep_work_minutes" => c.deep_work_minutes as f64,
        "commute_minutes" => c.commute_minutes as f64,
        "countdown_days" => c.countdown_days as f64,
        "distraction_minutes" => c.distraction_minutes as f64,
        "break_minutes" => c.break_minutes as f64,
        "evening_recap_time" => c.evening_recap_min as f64,
        "morning_recap_time" => c.morning_recap_min as f64,
        _ => 0.0,
    }
}

pub fn set_num(c: &mut Config, key: &str, v: f64) {
    match key {
        "notification_seconds" => c.notification_ms = (v * 1000.0).round() as i64,
        "osd_seconds" => c.osd_ms = (v * 1000.0).round() as i64,
        "low_battery_percent" => c.low_battery = v as f32,
        "meeting_lead_minutes" => c.meeting_lead_ms = (v * 60_000.0).round() as i64,
        "meeting_pill_minutes" => c.meeting_pill_ms = (v * 60_000.0).round() as i64,
        "travel_margin_minutes" => c.travel_margin_ms = (v * 60_000.0).round() as i64,
        "mini_pills" => c.mini_pills = (v.round() as u32).clamp(1, 4),
        "dock_icons" => c.dock_icons = (v.round() as u32).clamp(4, 12),
        "deep_work_minutes" => c.deep_work_minutes = v.round() as i64,
        "commute_minutes" => c.commute_minutes = v.round() as i64,
        "countdown_days" => c.countdown_days = (v.round() as u32).min(365),
        "distraction_minutes" => c.distraction_minutes = (v.round() as u32).min(120),
        "break_minutes" => c.break_minutes = v.round() as i64,
        "evening_recap_time" => c.evening_recap_min = (v.round() as i64).clamp(0, 1439),
        "morning_recap_time" => c.morning_recap_min = (v.round() as i64).clamp(0, 1439),
        _ => {}
    }
}

/// How a number is written: whole numbers plain, the others with one decimal.
pub fn fmt_num(v: f64, unit: &str) -> String {
    // a time of day, in minutes after midnight; 0 is "off"
    if unit == "time" {
        let m = v.round() as i64;
        return if m <= 0 { "off".to_string() } else { format!("{:02}:{:02}", m / 60, m % 60) };
    }
    if (v - v.round()).abs() < 0.005 { format!("{} {unit}", v.round() as i64) } else { format!("{v:.1} {unit}") }
}

/// Height of a tab's content (below the tabs): rows of 34.
/// The rules tab has a row per rule (at most four), a hint when there are none, and two rows to add one.
pub fn content_height(tab: usize, rules: usize) -> f64 {
    if tab == RULES_TAB {
        return (rules.clamp(1, 4) + 2) as f64 * 34.0;
    }
    let tg = toggles(tab).len().div_ceil(2);
    let help = HELP_H;
    let st = steppers(tab).len().div_ceil(2);
    (tg + st + text_rows(tab)) as f64 * 34.0 + help
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_option_on_a_tab_reads_and_writes_its_field() {
        let mut c = Config::default();
        for tab in 0..TABS.len() {
            for (key, label) in toggles(tab) {
                assert!(!label.is_empty());
                let before = get_bool(&c, key);
                set_bool(&mut c, key, !before);
                assert_eq!(get_bool(&c, key), !before, "{key} is wired to nothing");
                set_bool(&mut c, key, before);
            }
            for (key, _, _, step, min, max) in steppers(tab) {
                assert!(min < max && *step > 0.0);
                set_num(&mut c, key, *max);
                assert_eq!(get_num(&c, key), *max, "{key}");
                set_num(&mut c, key, *min);
                assert_eq!(get_num(&c, key), *min, "{key}");
            }
        }
    }

    #[test]
    fn every_option_here_is_one_the_platform_layer_can_write() {
        // the keys of the page and the keys the configuration accepts are the same list
        let page: std::collections::BTreeSet<&str> = (0..TABS.len()).flat_map(|t| toggles(t).iter().map(|(k, _)| *k)).collect();
        assert_eq!(page.len(), 34, "no option on two tabs");
        assert_eq!(TABS[RULES_TAB], "Rules");
        let nums: Vec<&str> = (0..TABS.len()).flat_map(|t| steppers(t).iter().map(|s| s.0)).collect();
        assert_eq!(nums, ["mini_pills", "dock_icons", "notification_seconds", "osd_seconds", "low_battery_percent", "meeting_lead_minutes", "meeting_pill_minutes", "deep_work_minutes", "break_minutes", "distraction_minutes", "morning_recap_time", "evening_recap_time", "commute_minutes", "travel_margin_minutes", "countdown_days"]);
    }

    #[test]
    fn every_option_says_what_it_controls() {
        let mut keys: Vec<&str> = (0..TABS.len()).flat_map(|t| toggles(t).iter().map(|(k, _)| *k).chain(steppers(t).iter().map(|s| s.0))).collect();
        keys.extend(["timer_presets", "screen", "map_tile_url"]);
        for k in keys {
            let d = describe(k);
            assert!(!d.is_empty(), "{k} has no description");
            assert!(d.chars().count() <= 76, "{k}: one line of the page");
        }
        assert_eq!(describe("nonsense"), "");
    }

    #[test]
    fn numbers_are_written_plainly() {
        assert_eq!(fmt_num(1110.0, "time"), "18:30");
        assert_eq!(fmt_num(0.0, "time"), "off");
        assert_eq!(fmt_num(5.0, "s"), "5 s");
        assert_eq!(fmt_num(1.6, "s"), "1.6 s");
        assert_eq!(fmt_num(15.0, "%"), "15 %");
    }
}
