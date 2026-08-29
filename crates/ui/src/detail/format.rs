//! Pure formatting logic for the commit detail panel.
//!
//! Nothing here touches gpui's `App` or `Window`: every function is a plain transformation
//! from domain types to strings, so it is testable without a running window.

use domain::{ObjectId, Timestamp};

/// Hexadecimal characters kept when an identifier is shown abbreviated, matching Git's
/// own default abbreviation length.
pub const ABBREV_LEN: usize = 7;

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub fn abbreviate(id: ObjectId) -> String {
    id.to_hex_prefix(ABBREV_LEN)
}

/// Days since the Unix epoch to a proleptic-Gregorian `(year, month, day)`.
///
/// Howard Hinnant's `civil_from_days` algorithm:
/// <https://howardhinnant.github.io/date_algorithms.html>.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day)
}

/// Renders `timestamp` the way `git log` does: in the author's own recorded UTC offset,
/// never converted to the viewer's local time.
pub fn format_timestamp(timestamp: Timestamp) -> String {
    let adjusted = timestamp.seconds + i64::from(timestamp.offset_minutes) * 60;
    let days = adjusted.div_euclid(86_400);
    let time_of_day = adjusted.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = time_of_day / 3_600;
    let minute = (time_of_day % 3_600) / 60;
    let second = time_of_day % 60;
    let month_name = MONTH_NAMES[(month - 1) as usize];
    format!("{day} {month_name} {year} at {hour:02}:{minute:02}:{second:02}")
}

/// Backslash-escapes every ASCII punctuation character, CommonMark's own escapable set.
///
/// A commit subject, identifier or author line is plain text, not Markdown source, but
/// the detail panel renders it through [`gpui_component::text::markdown`] to get
/// selectable text. Without this, a subject like `fix_bug` or `[WIP] release` would pick
/// up italics or link syntax it never asked for; escaping every ASCII punctuation
/// character, regardless of position, is what CommonMark guarantees always renders as a
/// literal character.
pub fn escape_markdown(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii_punctuation() {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(nibble: char) -> ObjectId {
        nibble.to_string().repeat(40).parse().unwrap()
    }

    #[test]
    fn abbreviate_truncates_to_seven_hex_characters() {
        assert_eq!(abbreviate(id('a')), "aaaaaaa");
    }

    #[test]
    fn the_epoch_renders_at_a_zero_offset() {
        let timestamp = Timestamp {
            seconds: 0,
            offset_minutes: 0,
        };
        assert_eq!(format_timestamp(timestamp), "1 January 1970 at 00:00:00");
    }

    #[test]
    fn a_known_anchor_renders_correctly_at_a_zero_offset() {
        let timestamp = Timestamp {
            seconds: 946_684_800,
            offset_minutes: 0,
        };
        assert_eq!(format_timestamp(timestamp), "1 January 2000 at 00:00:00");
    }

    #[test]
    fn a_positive_offset_advances_the_wall_clock_without_touching_the_instant() {
        let timestamp = Timestamp {
            seconds: 946_684_800,
            offset_minutes: 180,
        };
        assert_eq!(format_timestamp(timestamp), "1 January 2000 at 03:00:00");
    }

    #[test]
    fn a_negative_offset_can_move_the_wall_clock_into_the_previous_year() {
        let timestamp = Timestamp {
            seconds: 946_684_800,
            offset_minutes: -300,
        };
        assert_eq!(format_timestamp(timestamp), "31 December 1999 at 19:00:00");
    }

    #[test]
    fn escape_markdown_neutralises_ascii_punctuation_without_touching_letters_digits_or_spaces() {
        assert_eq!(
            escape_markdown("fix_bug 100% [WIP] (v1)"),
            "fix\\_bug 100\\% \\[WIP\\] \\(v1\\)"
        );
    }

    #[test]
    fn escape_markdown_escapes_a_literal_backslash_first() {
        assert_eq!(escape_markdown("a\\b"), "a\\\\b");
    }

    #[test]
    fn escape_markdown_leaves_non_ascii_text_untouched() {
        assert_eq!(
            escape_markdown("caf\u{e9} \u{2014} r\u{e9}sum\u{e9}"),
            "caf\u{e9} \u{2014} r\u{e9}sum\u{e9}"
        );
    }
}
