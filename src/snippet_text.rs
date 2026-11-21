//! 片段正文里的动态占位符：`{date}`、`{time}`、`{datetime}`、`{weekday}`，
//! 以及带格式的 `{date format="yyyy-MM-dd"}`。

use jiff::Timestamp;
use jiff::tz::TimeZone;
use nsh_core::platform::HourCycle;
use nsh_core::prelude::t;

/// 正文切分后的一段：普通文本，或已经换成真实值的占位符。
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Text(String),
    Value(String),
}

/// 把正文展开成纯文本（占位符替换成当前时间）。
pub fn expand(content: &str) -> String {
    render(content)
        .into_iter()
        .map(|piece| match piece {
            Piece::Text(text) | Piece::Value(text) => text,
        })
        .collect()
}

/// 把正文切成文本与占位符值，供预览高亮显示。
pub fn render(content: &str) -> Vec<Piece> {
    let now = Timestamp::now().to_zoned(TimeZone::system());
    render_at(content, &now, chinese(), hour24())
}

/// 供测试用的版本：时间、语言与小时制都由调用方给。
fn render_at(content: &str, now: &jiff::Zoned, chinese: bool, hour24: bool) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut rest = content;

    while let Some(start) = rest.find('{') {
        text.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(end) = after.find('}') else {
            break;
        };
        let token = &after[1..end];
        match resolve(token, now, chinese, hour24) {
            Some(value) => {
                if !text.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut text)));
                }
                pieces.push(Piece::Value(value));
                rest = &after[end + 1..];
            }
            None => {
                // 不是占位符：把 `{` 当普通文本继续找。
                text.push('{');
                rest = &after[1..];
            }
        }
    }
    text.push_str(rest);
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    pieces
}

fn resolve(token: &str, now: &jiff::Zoned, chinese: bool, hour24: bool) -> Option<String> {
    let (name, format) = split_token(token);
    let name = name.trim().to_ascii_lowercase();
    let custom = format.map(|format| convert_format(&format));
    match name.as_str() {
        "date" => Some(match custom {
            Some(format) => now.strftime(&format).to_string(),
            None => date_default(now, chinese),
        }),
        "time" => Some(match custom {
            Some(format) => now.strftime(&format).to_string(),
            None => time_of_day(now, hour24),
        }),
        "datetime" => Some(match custom {
            Some(format) => now.strftime(&format).to_string(),
            None => format!("{} {}", date_default(now, chinese), time_of_day(now, hour24)),
        }),
        "weekday" => Some(match custom {
            Some(format) => now.strftime(&format).to_string(),
            None => weekday(now),
        }),
        _ => None,
    }
}

/// `date format="yyyy-MM-dd"` → (`date`, Some(`yyyy-MM-dd`))。
fn split_token(token: &str) -> (&str, Option<String>) {
    let token = token.trim();
    let lower = token.to_ascii_lowercase();
    let Some(at) = lower.find("format") else {
        return (token, None);
    };
    let name = token[..at].trim();
    let rest = token[at + "format".len()..].trim_start();
    let Some(rest) = rest.strip_prefix('=') else {
        return (token, None);
    };
    let value = rest.trim_start().trim_matches(|c| c == '"' || c == '\'');
    (name, Some(value.to_string()))
}

/// ICU 风格（`yyyy-MM-dd`）到 `strftime` 的常用映射。
fn convert_format(pattern: &str) -> String {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        let mut run = 1;
        while chars.peek() == Some(&ch) {
            chars.next();
            run += 1;
        }
        let piece = match (ch, run) {
            ('y', 4) => "%Y".to_string(),
            ('y', 2) => "%y".to_string(),
            ('M', 1) => "%-m".to_string(),
            ('M', 2) => "%m".to_string(),
            ('M', 3) => "%b".to_string(),
            ('M', n) if n >= 4 => "%B".to_string(),
            ('d', 1) => "%-d".to_string(),
            ('d', _) => "%d".to_string(),
            ('H', 1) => "%-H".to_string(),
            ('H', _) => "%H".to_string(),
            ('h', 1) => "%-I".to_string(),
            ('h', _) => "%I".to_string(),
            ('m', _) => "%M".to_string(),
            ('s', _) => "%S".to_string(),
            ('E', n) if n >= 4 => "%A".to_string(),
            ('E', _) => "%a".to_string(),
            ('a', _) => "%p".to_string(),
            _ => ch.to_string().repeat(run),
        };
        out.push_str(&piece);
    }
    out
}

fn hour24() -> bool {
    matches!(
        nsh_core::element::locale_settings().hour_cycle,
        HourCycle::H23 | HourCycle::H24
    )
}

fn time_of_day(now: &jiff::Zoned, hour24: bool) -> String {
    if hour24 {
        now.strftime("%H:%M").to_string()
    } else {
        now.strftime("%I:%M %p").to_string()
    }
}

fn date_default(now: &jiff::Zoned, chinese: bool) -> String {
    if chinese {
        now.strftime("%Y-%m-%d").to_string()
    } else {
        now.strftime("%b %d, %Y").to_string()
    }
}

fn weekday(now: &jiff::Zoned) -> String {
    let names = t!("weekday.names");
    let index = match now.weekday() {
        jiff::civil::Weekday::Monday => 0,
        jiff::civil::Weekday::Tuesday => 1,
        jiff::civil::Weekday::Wednesday => 2,
        jiff::civil::Weekday::Thursday => 3,
        jiff::civil::Weekday::Friday => 4,
        jiff::civil::Weekday::Saturday => 5,
        jiff::civil::Weekday::Sunday => 6,
    };
    names
        .split(',')
        .nth(index)
        .map(|name| name.trim().to_string())
        .unwrap_or_else(|| now.strftime("%A").to_string())
}

fn chinese() -> bool {
    nsh_core::element::locale().language() == "zh"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(content: &str) -> Vec<&'static str> {
        let now = Timestamp::now().to_zoned(TimeZone::UTC);
        render_at(content, &now, false, true)
            .iter()
            .map(|piece| match piece {
                Piece::Text(_) => "text",
                Piece::Value(_) => "value",
            })
            .collect()
    }

    #[test]
    fn plain_text_is_one_piece() {
        assert_eq!(kinds("hello"), ["text"]);
    }

    #[test]
    fn known_placeholders_become_values() {
        assert_eq!(
            kinds("a {date} b {time} c"),
            ["text", "value", "text", "value", "text"]
        );
    }

    #[test]
    fn unknown_braces_stay_text() {
        assert_eq!(kinds("a {nope} b"), ["text"]);
    }

    #[test]
    fn custom_formats_are_converted() {
        assert_eq!(convert_format("yyyy-MM-dd"), "%Y-%m-%d");
        assert_eq!(convert_format("HH:mm:ss"), "%H:%M:%S");
        assert_eq!(convert_format("EEEE"), "%A");
    }

    #[test]
    fn expand_removes_placeholders() {
        let now = Timestamp::now().to_zoned(TimeZone::UTC);
        let pieces = render_at("{date} {time}", &now, false, true);
        assert_eq!(pieces.len(), 3);
        assert!(pieces.iter().any(|piece| matches!(piece, Piece::Value(_))));
    }
}
