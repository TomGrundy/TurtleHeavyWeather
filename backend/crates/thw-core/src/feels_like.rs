use crate::models::WeatherPoint;

/// Computes Australian Apparent Temperature using the Bureau of Meteorology
/// Steadman formula (shade, ignoring net radiation):
///   e  = (rh / 100) * 6.105 * exp(17.27 * Ta / (237.7 + Ta))
///   AT = Ta + 0.33 * e - 0.70 * ws - 4.00
/// where `ws` is wind speed in m/s (`wind_kmh / 3.6`).
pub fn compute_apparent_temp_c(temp_c: f64, rel_humidity_pct: f64, wind_kmh: f64) -> f64 {
    let rh = rel_humidity_pct.clamp(0.0, 100.0);
    let ws_ms = (wind_kmh.max(0.0)) / 3.6;
    let vapor_pressure = (rh / 100.0) * 6.105 * ((17.27 * temp_c) / (237.7 + temp_c)).exp();
    let at = temp_c + 0.33 * vapor_pressure - 0.70 * ws_ms - 4.00;
    (at * 10.0).round() / 10.0
}

/// Fills `feels_like_c` using `compute_apparent_temp_c` if missing and sufficient
/// inputs are present, marking `feels_like_is_derived = true`.
pub fn ensure_feels_like(point: &mut WeatherPoint) {
    if point.feels_like_c.is_some() {
        return;
    }
    if let Some(temp_c) = point.temp_c {
        if let Some(rh) = point.humidity_pct {
            let wind = point.wind_kmh.unwrap_or(0.0);
            point.feels_like_c = Some(compute_apparent_temp_c(temp_c, rh, wind));
            point.feels_like_is_derived = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn bom_steadman_apparent_temp_matches_reference() {
        // At 25°C, 60% RH, 18 km/h (5 m/s):
        // e = 0.6 * 6.105 * exp(17.27 * 25 / 262.7) ~= 18.99
        // AT = 25 + 0.33*18.99 - 0.70*5 - 4.00 ~= 23.8°C
        let at = compute_apparent_temp_c(25.0, 60.0, 18.0);
        assert!((at - 23.8).abs() <= 0.15, "got {at}");

        // Hot humid calm day: 32°C, 75% RH, 3.6 km/h (1 m/s) -> hotter than air temp
        let hot = compute_apparent_temp_c(32.0, 75.0, 3.6);
        assert!(hot > 35.0, "expected muggy feels-like > 35, got {hot}");
    }

    #[test]
    fn ensure_feels_like_preserves_native_and_derives_missing() {
        let mut native = WeatherPoint {
            valid_at: Utc::now(),
            temp_c: Some(20.0),
            feels_like_c: Some(19.1),
            feels_like_is_derived: false,
            rain_prob_pct: None,
            rain_mm: None,
            wind_kmh: Some(15.0),
            humidity_pct: Some(55.0),
            summary: None,
        };
        ensure_feels_like(&mut native);
        assert_eq!(native.feels_like_c, Some(19.1));
        assert!(!native.feels_like_is_derived);

        let mut derived = WeatherPoint {
            feels_like_c: None,
            ..native
        };
        ensure_feels_like(&mut derived);
        assert!(derived.feels_like_c.is_some());
        assert!(derived.feels_like_is_derived);
    }
}
