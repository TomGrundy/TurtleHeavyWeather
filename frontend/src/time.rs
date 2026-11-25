use chrono::{DateTime, Utc};

#[cfg(target_arch = "wasm32")]
fn js_date(dt: DateTime<Utc>) -> js_sys::Date {
    js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(
        dt.timestamp_millis() as f64
    ))
}

#[cfg(target_arch = "wasm32")]
fn month_abbr(m: u32) -> &'static str {
    match m {
        0 => "Jan",
        1 => "Feb",
        2 => "Mar",
        3 => "Apr",
        4 => "May",
        5 => "Jun",
        6 => "Jul",
        7 => "Aug",
        8 => "Sep",
        9 => "Oct",
        10 => "Nov",
        11 => "Dec",
        _ => "",
    }
}

#[cfg(target_arch = "wasm32")]
fn to_12h(hour24: u32) -> (u32, &'static str) {
    let ampm = if hour24 < 12 { "AM" } else { "PM" };
    let h12 = match hour24 % 12 {
        0 => 12,
        h => h,
    };
    (h12, ampm)
}

/// Formats `dt` as `h:mm AM/PM` in the browser's local timezone.
pub fn format_local_hhmm(dt: DateTime<Utc>) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let d = js_date(dt);
        let (h12, ampm) = to_12h(d.get_hours());
        format!("{}:{:02} {}", h12, d.get_minutes(), ampm)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        dt.with_timezone(&chrono::Local)
            .format("%-I:%M %p")
            .to_string()
    }
}

/// Formats `dt` as `DD Mon h:mm AM/PM` in the browser's local timezone.
pub fn format_local_short(dt: DateTime<Utc>) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let d = js_date(dt);
        let (h12, ampm) = to_12h(d.get_hours());
        format!(
            "{:02} {} {}:{:02} {}",
            d.get_date(),
            month_abbr(d.get_month()),
            h12,
            d.get_minutes(),
            ampm
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        dt.with_timezone(&chrono::Local)
            .format("%d %b %-I:%M %p")
            .to_string()
    }
}

/// Formats `dt` as `DD Mon YYYY h:mm AM/PM` in the browser's local timezone.
pub fn format_local_full(dt: DateTime<Utc>) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let d = js_date(dt);
        let (h12, ampm) = to_12h(d.get_hours());
        format!(
            "{:02} {} {} {}:{:02} {}",
            d.get_date(),
            month_abbr(d.get_month()),
            d.get_full_year(),
            h12,
            d.get_minutes(),
            ampm
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        dt.with_timezone(&chrono::Local)
            .format("%d %b %Y %-I:%M %p")
            .to_string()
    }
}
