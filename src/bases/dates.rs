use super::value::Value;
use anyhow::{Context, bail};
use chrono::{
    DateTime, Datelike, Days, Duration, Local, LocalResult, Months, NaiveDate, NaiveDateTime,
    TimeZone, Timelike,
};

pub fn local(date: NaiveDateTime) -> anyhow::Result<DateTime<Local>> {
    match Local.from_local_datetime(&date) {
        LocalResult::Single(v) => Ok(v),
        LocalResult::Ambiguous(a, _) => Ok(a), // Moment chooses the earlier offset.
        LocalResult::None => {
            bail!("local datetime does not exist due to timezone transition: {date}")
        }
    }
}
pub fn parse_date(text: &str) -> anyhow::Result<DateTime<Local>> {
    if let Ok(v) = DateTime::parse_from_rfc3339(text) {
        return Ok(v.with_timezone(&Local));
    }
    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(v) = NaiveDateTime::parse_from_str(text, format) {
            return local(v);
        }
    }
    if let Ok(v) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return local(v.and_hms_opt(0, 0, 0).unwrap());
    }
    bail!("invalid date {text:?}; expected YYYY-MM-DD[ HH:mm:ss] or ISO 8601 datetime")
}
pub fn value(text: &str) -> anyhow::Result<Value> {
    let date = parse_date(text)?;
    Ok(if text.len() == 10 {
        Value::DateOnly(date)
    } else {
        Value::Date(date)
    })
}
pub fn duration(text: &str) -> anyhow::Result<Value> {
    let regex=regex::Regex::new(r"([+-]?(?:\d+(?:\.\d*)?|\.\d+))\s*(milliseconds?|seconds?|minutes?|hours?|days?|weeks?|months?|years?|ms|[smhdwMy])").unwrap();
    let mut months = 0.0;
    let mut days = 0.0;
    let mut milliseconds = 0.0;
    let mut previous = 0;
    let mut found = false;
    for capture in regex.captures_iter(text) {
        let m = capture.get(0).unwrap();
        if !text[previous..m.start()].trim().is_empty() {
            bail!("invalid duration: {text:?}");
        }
        previous = m.end();
        found = true;
        let n: f64 = capture[1].parse()?;
        match &capture[2] {
            "y" | "year" | "years" => months += n * 12.0,
            "M" | "month" | "months" => months += n,
            "w" | "week" | "weeks" => days += n * 7.0,
            "d" | "day" | "days" => days += n,
            "h" | "hour" | "hours" => milliseconds += n * 3600000.0,
            "m" | "minute" | "minutes" => milliseconds += n * 60000.0,
            "s" | "second" | "seconds" => milliseconds += n * 1000.0,
            _ => milliseconds += n,
        }
    }
    if !found || !text[previous..].trim().is_empty() {
        bail!("invalid duration: {text:?}");
    }
    Ok(Value::Duration {
        months,
        days,
        milliseconds,
    })
}
pub fn add(
    date: DateTime<Local>,
    months: f64,
    days: f64,
    milliseconds: f64,
) -> anyhow::Result<DateTime<Local>> {
    if !months.is_finite()
        || !days.is_finite()
        || !milliseconds.is_finite()
        || months.abs() > u32::MAX as f64
        || days.abs() > i64::MAX as f64
        || milliseconds.abs() > i64::MAX as f64
    {
        bail!("date duration is out of range");
    }
    let months = months.round() as i64;
    let date = if months < 0 {
        date.checked_sub_months(Months::new((-months) as u32))
    } else {
        date.checked_add_months(Months::new(months as u32))
    }
    .context("date month arithmetic is out of range")?;
    let days = days.round() as i64;
    let date = if days < 0 {
        date.checked_sub_days(Days::new(days.unsigned_abs()))
    } else {
        date.checked_add_days(Days::new(days as u64))
    }
    .context("calendar day arithmetic is out of range or lands in a nonexistent local time")?;
    date.checked_add_signed(Duration::milliseconds(milliseconds.round() as i64))
        .context("date arithmetic is out of range")
}
pub fn field(date: DateTime<Local>, name: &str) -> anyhow::Result<Value> {
    let v = match name {
        "year" => date.year() as f64,
        "month" => date.month() as f64,
        "day" => date.day() as f64,
        "hour" => date.hour() as f64,
        "minute" => date.minute() as f64,
        "second" => date.second() as f64,
        "millisecond" => date.timestamp_subsec_millis() as f64,
        _ => bail!("unknown date field: {name}"),
    };
    Ok(Value::Number(v))
}
fn ordinal(n: u32) -> String {
    format!(
        "{n}{}",
        if (11..=13).contains(&(n % 100)) {
            "th"
        } else {
            match n % 10 {
                1 => "st",
                2 => "nd",
                3 => "rd",
                _ => "th",
            }
        }
    )
}
fn week(date: DateTime<Local>) -> (i32, u32) {
    // English Moment locale: Sunday is day 0; week containing Jan 1 is week 1.
    let year = date.year();
    let start = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let next = NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap();
    let next_start = next - Duration::days(next.weekday().num_days_from_sunday() as i64);
    if date.date_naive() >= next_start {
        return (year + 1, 1);
    }
    let start = start - Duration::days(start.weekday().num_days_from_sunday() as i64);
    (
        year,
        ((date.date_naive() - start).num_days() / 7 + 1) as u32,
    )
}
pub fn format(date: DateTime<Local>, pattern: &str) -> anyhow::Result<String> {
    const MONTHS: [&str; 12] = [
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
    const DAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    const TOKENS: &[&str] = &[
        "YYYYYY",
        "YYYYY",
        "YYYY",
        "YY",
        "Y",
        "MMMM",
        "MMM",
        "MM",
        "Mo",
        "M",
        "Qo",
        "Q",
        "DDDo",
        "DDDD",
        "DDD",
        "Do",
        "DD",
        "D",
        "dddd",
        "ddd",
        "dd",
        "do",
        "d",
        "e",
        "E",
        "ww",
        "wo",
        "w",
        "WW",
        "Wo",
        "W",
        "gggg",
        "gg",
        "GGGG",
        "GG",
        "Hmmss",
        "Hmm",
        "hmmss",
        "hmm",
        "HH",
        "H",
        "hh",
        "h",
        "kk",
        "k",
        "mm",
        "m",
        "ss",
        "s",
        "SSSSSSSSS",
        "SSSSSSSS",
        "SSSSSSS",
        "SSSSSS",
        "SSSSS",
        "SSSS",
        "SSS",
        "SS",
        "S",
        "A",
        "a",
        "ZZ",
        "Z",
        "zz",
        "z",
        "X",
        "x",
        "LTS",
        "LT",
        "LLLL",
        "LLL",
        "LL",
        "L",
        "llll",
        "lll",
        "ll",
        "l",
    ];
    let mut rest = pattern;
    let mut out = String::new();
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix('[') {
            let Some(end) = tail.find(']') else {
                bail!("unclosed Moment format literal");
            };
            out.push_str(&tail[..end]);
            rest = &tail[end + 1..];
            continue;
        }
        if let Some(tail) = rest.strip_prefix('\\') {
            let Some(ch) = tail.chars().next() else {
                bail!("unfinished Moment format escape");
            };
            out.push(ch);
            rest = &tail[ch.len_utf8()..];
            continue;
        }
        let Some(token) = TOKENS.iter().find(|token| rest.starts_with(**token)) else {
            let ch = rest.chars().next().unwrap();
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
            continue;
        };
        rest = &rest[token.len()..];
        let iso = date.iso_week();
        let (wy, wn) = week(date);
        let hour12 = if date.hour().is_multiple_of(12) {
            12
        } else {
            date.hour() % 12
        };
        let day = date.weekday().num_days_from_sunday();
        let value = match *token {
            "Y" => date.year().to_string(),
            "YY" => format!("{:02}", date.year().rem_euclid(100)),
            "YYYY" => format!("{:04}", date.year()),
            "YYYYY" => format!("{:05}", date.year()),
            "YYYYYY" => format!("{:+07}", date.year()),
            "M" => date.month().to_string(),
            "MM" => format!("{:02}", date.month()),
            "Mo" => ordinal(date.month()),
            "MMM" => MONTHS[date.month0() as usize][..3].into(),
            "MMMM" => MONTHS[date.month0() as usize].into(),
            "Q" => (date.month0() / 3 + 1).to_string(),
            "Qo" => ordinal(date.month0() / 3 + 1),
            "D" => date.day().to_string(),
            "DD" => format!("{:02}", date.day()),
            "Do" => ordinal(date.day()),
            "DDD" => date.ordinal().to_string(),
            "DDDD" => format!("{:03}", date.ordinal()),
            "DDDo" => ordinal(date.ordinal()),
            "d" | "e" => day.to_string(),
            "do" => ordinal(day),
            "dd" => DAYS[day as usize][..2].into(),
            "ddd" => DAYS[day as usize][..3].into(),
            "dddd" => DAYS[day as usize].into(),
            "E" => date.weekday().number_from_monday().to_string(),
            "w" => wn.to_string(),
            "ww" => format!("{wn:02}"),
            "wo" => ordinal(wn),
            "W" => iso.week().to_string(),
            "WW" => format!("{:02}", iso.week()),
            "Wo" => ordinal(iso.week()),
            "gg" => format!("{:02}", wy.rem_euclid(100)),
            "gggg" => wy.to_string(),
            "GG" => format!("{:02}", iso.year().rem_euclid(100)),
            "GGGG" => iso.year().to_string(),
            "H" => date.hour().to_string(),
            "HH" => format!("{:02}", date.hour()),
            "h" => hour12.to_string(),
            "hh" => format!("{hour12:02}"),
            "k" => {
                if date.hour() == 0 {
                    "24".into()
                } else {
                    date.hour().to_string()
                }
            }
            "kk" => format!("{:02}", if date.hour() == 0 { 24 } else { date.hour() }),
            "Hmm" => format!("{}{:02}", date.hour(), date.minute()),
            "Hmmss" => format!("{}{:02}{:02}", date.hour(), date.minute(), date.second()),
            "hmm" => format!("{hour12}{:02}", date.minute()),
            "hmmss" => format!("{hour12}{:02}{:02}", date.minute(), date.second()),
            "m" => date.minute().to_string(),
            "mm" => format!("{:02}", date.minute()),
            "s" => date.second().to_string(),
            "ss" => format!("{:02}", date.second()),
            "a" => {
                if date.hour() < 12 {
                    "am".into()
                } else {
                    "pm".into()
                }
            }
            "A" => {
                if date.hour() < 12 {
                    "AM".into()
                } else {
                    "PM".into()
                }
            }
            "Z" => date.format("%:z").to_string(),
            "ZZ" => date.format("%z").to_string(),
            "z" | "zz" => String::new(),
            "X" => (date.timestamp_millis() as f64 / 1000.0).to_string(),
            "x" => date.timestamp_millis().to_string(),
            "LT" => format(date, "h:mm A")?,
            "LTS" => format(date, "h:mm:ss A")?,
            "L" => format(date, "MM/DD/YYYY")?,
            "l" => format(date, "M/D/YYYY")?,
            "LL" => format(date, "MMMM D, YYYY")?,
            "ll" => format(date, "MMM D, YYYY")?,
            "LLL" => format(date, "MMMM D, YYYY h:mm A")?,
            "lll" => format(date, "MMM D, YYYY h:mm A")?,
            "LLLL" => format(date, "dddd, MMMM D, YYYY h:mm A")?,
            "llll" => format(date, "ddd, MMM D, YYYY h:mm A")?,
            token if token.starts_with('S') => {
                let digits = format!("{:09}", date.timestamp_subsec_nanos());
                digits[..token.len()].into()
            }
            _ => unreachable!(),
        };
        out.push_str(&value);
    }
    Ok(out)
}
pub fn relative(date: DateTime<Local>, now: DateTime<Local>) -> String {
    let diff = date.signed_duration_since(now).num_milliseconds();
    let phrase = humanize(0.0, diff as f64);
    if diff > 0 {
        format!("in {phrase}")
    } else {
        format!("{phrase} ago")
    }
}
pub fn humanize(duration_months: f64, milliseconds: f64) -> String {
    let seconds =
        ((duration_months * 146097.0 / 4800.0 * 86400000.0 + milliseconds).abs() / 1000.0).round();
    let minutes = (seconds / 60.0).round();
    let hours = (minutes / 60.0).round();
    let days = (hours / 24.0).round();
    let months = (days * 4800.0 / 146097.0).round();
    let years = (months / 12.0).round();
    if seconds < 45.0 {
        "a few seconds".into()
    } else if minutes <= 1.0 {
        "a minute".into()
    } else if minutes < 45.0 {
        format!("{minutes} minutes")
    } else if hours <= 1.0 {
        "an hour".into()
    } else if hours < 22.0 {
        format!("{hours} hours")
    } else if days <= 1.0 {
        "a day".into()
    } else if days < 26.0 {
        format!("{days} days")
    } else if months <= 1.0 {
        "a month".into()
    } else if months < 11.0 {
        format!("{months} months")
    } else if years <= 1.0 {
        "a year".into()
    } else {
        format!("{years} years")
    }
}
