use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use quick_xml::events::Event;
use quick_xml::Reader;
use thw_core::{
    classify_uv_category, haversine_km, MeshNode, SourceKind, SourceReport, WeatherPoint,
};

use crate::{FetchContext, SourceError, SourceOutput, WeatherSource};

pub const DEFAULT_ARPANSA_UV_URL: &str = "https://uvdata.arpansa.gov.au/xml/uvvalues.xml";
pub const SOURCE_ID_ARPANSA_UV: &str = "arpansa_uv";

#[derive(Debug, Clone, PartialEq)]
pub struct ArpansaObservation {
    pub station_name: String,
    pub code: String,
    pub index: f64,
    pub observed_at: DateTime<Utc>,
    pub lat: f64,
    pub lon: f64,
}

/// WeatherSource adapter for the Australian Radiation Protection and Nuclear Safety Agency
/// (ARPANSA) live UV index XML feed.
#[derive(Debug, Clone)]
pub struct ArpansaUvSource {
    url: String,
    enabled: bool,
}

impl Default for ArpansaUvSource {
    fn default() -> Self {
        Self {
            url: DEFAULT_ARPANSA_UV_URL.to_string(),
            enabled: true,
        }
    }
}

impl ArpansaUvSource {
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[async_trait]
impl WeatherSource for ArpansaUvSource {
    fn source_id(&self) -> &'static str {
        SOURCE_ID_ARPANSA_UV
    }

    fn display_name(&self) -> &'static str {
        "ARPANSA Live UV Index"
    }

    fn default_weight(&self) -> f64 {
        1.0
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn fetch_mesh(
        &self,
        nodes: &[MeshNode],
        ctx: &FetchContext,
    ) -> Result<SourceOutput, SourceError> {
        if !self.enabled || nodes.is_empty() {
            return Ok(SourceOutput::empty());
        }

        let client = reqwest::Client::builder()
            .user_agent(&ctx.user_agent)
            .timeout(Duration::from_secs(ctx.timeout_secs))
            .build()?;

        let xml = client
            .get(&self.url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let observations = parse_arpansa_xml(&xml)?;
        if observations.is_empty() {
            return Ok(SourceOutput::empty());
        }

        let fetched_at = Utc::now();
        let mut reports = Vec::with_capacity(nodes.len());

        for node in nodes {
            if let Some(obs) = nearest_station(&observations, node.lat, node.lon) {
                let category = classify_uv_category(obs.index);
                let summary = format!("UV {:.1} ({}) · {}", obs.index, category, obs.station_name);
                let report = SourceReport {
                    source_id: SOURCE_ID_ARPANSA_UV.to_string(),
                    display_name: self.display_name().to_string(),
                    kind: SourceKind::Observation,
                    fetched_at,
                    weight: self.default_weight(),
                    current: WeatherPoint {
                        valid_at: obs.observed_at,
                        temp_c: None,
                        feels_like_c: None,
                        feels_like_is_derived: false,
                        rain_prob_pct: None,
                        rain_mm: None,
                        wind_kmh: None,
                        humidity_pct: None,
                        solar_wm2: None,
                        summary: Some(summary),
                    },
                    hourly: Vec::new(),
                    source_url: Some(self.url.clone()),
                };
                reports.push((node.node_id.clone(), report));
            }
        }

        Ok(SourceOutput {
            reports,
            observations: Vec::new(),
        })
    }
}

fn australian_station_coords(station_id: &str) -> Option<(f64, f64)> {
    match station_id.trim().to_ascii_lowercase().as_str() {
        "adelaide" => Some((-34.9285, 138.6007)),
        "alice springs" => Some((-23.6980, 133.8807)),
        "brisbane" => Some((-27.4698, 153.0251)),
        "canberra" => Some((-35.2809, 149.1300)),
        "darwin" => Some((-12.4634, 130.8456)),
        "emerald" => Some((-23.5273, 148.1608)),
        "gold coast" => Some((-28.0167, 153.4000)),
        "kingston" => Some((-42.9772, 147.3083)),
        "melbourne" => Some((-37.7161, 145.1030)), // Yallambie / Melbourne ARPANSA site
        "newcastle" => Some((-32.9283, 151.7817)),
        "perth" => Some((-31.9505, 115.8605)),
        "sydney" => Some((-34.0419, 151.1160)), // Miranda / Sydney ARPANSA site
        "townsville" => Some((-19.2590, 146.8169)),
        _ => None, // Excludes Antarctic stations (Casey, Davis, Mawson)
    }
}

pub fn parse_arpansa_xml(xml: &str) -> Result<Vec<ArpansaObservation>, SourceError> {
    let mut reader = Reader::from_str(xml.trim_start_matches('\u{feff}'));
    reader.config_mut().trim_text(true);

    let mut observations = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_tag: Option<String> = None;
    let mut code = String::new();
    let mut index_str = String::new();
    let mut utc_str = String::new();
    let mut status = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "location" {
                    current_id = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"id")
                        .and_then(|a| a.unescape_value().ok())
                        .map(|s| s.to_string());
                    code.clear();
                    index_str.clear();
                    utc_str.clear();
                    status.clear();
                } else if current_id.is_some() {
                    current_tag = Some(name);
                }
            }
            Ok(Event::Text(e)) => {
                if let Some(tag) = current_tag.as_deref() {
                    let txt = e
                        .unescape()
                        .map_err(|err| SourceError::Parse(err.to_string()))?
                        .to_string();
                    match tag {
                        "name" => code = txt,
                        "index" => index_str = txt,
                        "utcdatetime" => utc_str = txt,
                        "status" => status = txt,
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "location" {
                    if let Some(station_name) = current_id.take() {
                        if status.eq_ignore_ascii_case("ok") {
                            if let (Some((lat, lon)), Ok(index), Ok(naive)) = (
                                australian_station_coords(&station_name),
                                index_str.trim().parse::<f64>(),
                                NaiveDateTime::parse_from_str(utc_str.trim(), "%Y/%m/%d %H:%M"),
                            ) {
                                observations.push(ArpansaObservation {
                                    station_name,
                                    code: code.clone(),
                                    index: (index * 10.0).round() / 10.0,
                                    observed_at: DateTime::from_naive_utc_and_offset(naive, Utc),
                                    lat,
                                    lon,
                                });
                            }
                        }
                    }
                }
                current_tag = None;
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(SourceError::Parse(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(observations)
}

fn nearest_station(
    observations: &[ArpansaObservation],
    lat: f64,
    lon: f64,
) -> Option<&ArpansaObservation> {
    observations.iter().min_by(|a, b| {
        let da = haversine_km(lat, lon, a.lat, a.lon);
        let db = haversine_km(lat, lon, b.lat, b.lon);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arpansa_xml_and_ignores_antarctica_and_bad_status() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<stations>
  <location id="Melbourne">
    <name>mel</name>
    <index>6.4</index>
    <utcdatetime>2026/11/26 03:59</utcdatetime>
    <status>ok</status>
  </location>
  <location id="Casey">
    <name>cas</name>
    <index>2.1</index>
    <utcdatetime>2026/11/26 03:59</utcdatetime>
    <status>ok</status>
  </location>
  <location id="Sydney">
    <name>syd</name>
    <index>8.2</index>
    <utcdatetime>2026/11/26 03:59</utcdatetime>
    <status>offline</status>
  </location>
</stations>"#;

        let parsed = parse_arpansa_xml(xml).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].station_name, "Melbourne");
        assert!((parsed[0].index - 6.4).abs() < 1e-6);
    }

    #[test]
    fn picks_nearest_australian_uv_station() {
        let obs = vec![
            ArpansaObservation {
                station_name: "Melbourne".into(),
                code: "mel".into(),
                index: 5.3,
                observed_at: Utc::now(),
                lat: -37.7161,
                lon: 145.1030,
            },
            ArpansaObservation {
                station_name: "Sydney".into(),
                code: "syd".into(),
                index: 7.8,
                observed_at: Utc::now(),
                lat: -34.0419,
                lon: 151.1160,
            },
        ];

        // Mont Albert North (-37.798, 145.110) -> Melbourne
        let chosen = nearest_station(&obs, -37.798, 145.110).unwrap();
        assert_eq!(chosen.station_name, "Melbourne");
    }
}
