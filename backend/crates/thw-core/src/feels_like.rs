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

/// Estimates outdoor sun-exposed Apparent Temperature by adding a bounded
/// Steadman solar radiation load (`+ 0.008 * shortwave_radiation_wm2`, capped
/// at +8.0°C above shade apparent temperature).
pub fn compute_outdoor_sun_apparent_temp_c(shade_at_c: f64, shortwave_radiation_wm2: f64) -> f64 {
    let solar_boost = (shortwave_radiation_wm2.max(0.0) * 0.008).min(8.0);
    ((shade_at_c + solar_boost) * 10.0).round() / 10.0
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

/// Standardizes a `WeatherPoint`'s `feels_like_c` to BOM's Australian Steadman
/// formula whenever `temp_c` and `humidity_pct` are available, eliminating
/// Heat-Index / Wind-Chill drift across US-centric APIs.
pub fn normalize_feels_like_bom_steadman(point: &mut WeatherPoint) {
    if let (Some(temp_c), Some(rh)) = (point.temp_c, point.humidity_pct) {
        let wind = point.wind_kmh.unwrap_or(0.0);
        point.feels_like_c = Some(compute_apparent_temp_c(temp_c, rh, wind));
        point.feels_like_is_derived = true;
    } else {
        ensure_feels_like(point);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn bom_steadman_apparent_temp_matches_reference() {
        let at = compute_apparent_temp_c(25.0, 60.0, 18.0);
        assert!((at - 23.8).abs() <= 0.15, "got {at}");

        let hot = compute_apparent_temp_c(32.0, 75.0, 3.6);
        assert!(hot > 35.0, "expected muggy feels-like > 35, got {hot}");
    }

    #[test]
    fn solar_boost_increases_apparent_temp_up_to_cap() {
        assert_eq!(compute_outdoor_sun_apparent_temp_c(24.0, 0.0), 24.0);
        assert_eq!(compute_outdoor_sun_apparent_temp_c(24.0, 500.0), 28.0);
        assert_eq!(compute_outdoor_sun_apparent_temp_c(24.0, 1500.0), 32.0);
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
            solar_wm2: None,
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
