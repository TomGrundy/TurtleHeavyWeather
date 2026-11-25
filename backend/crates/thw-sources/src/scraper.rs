use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use scraper::{ElementRef, Html, Selector};
use thw_core::{
    ensure_feels_like, MeshNode, ObservationRecord, SourceKind, SourceReport, StateCode,
    WeatherPoint,
};
use tracing::{info, warn};

use crate::{FetchContext, SourceError, SourceOutput, WeatherSource};

pub const DEFAULT_BOM_HTML_BASE: &str = "http://www.bom.gov.au";
const BOM_SCRAPER_QUALITY_RANK: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuleKind {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RobotsRule {
    kind: RuleKind,
    pattern: String,
}

#[derive(Debug, Clone, Default)]
struct AgentGroup {
    agents: Vec<String>,
    rules: Vec<RobotsRule>,
}

/// Parsed `robots.txt` policy supporting RFC 9309 `User-agent`, `Allow`, `Disallow`,
/// `*` wildcards, `$` end anchors, and longest-match specificity.
#[derive(Debug, Clone, Default)]
pub struct RobotsPolicy {
    groups: Vec<AgentGroup>,
}

impl RobotsPolicy {
    pub fn parse(robots_txt: &str) -> Self {
        let mut groups: Vec<AgentGroup> = Vec::new();
        let mut current = AgentGroup::default();
        let mut seen_directive_in_group = false;

        for raw_line in robots_txt.lines() {
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            let Some((raw_field, raw_value)) = line.split_once(':') else {
                continue;
            };
            let field = raw_field.trim().to_ascii_lowercase();
            let value = raw_value.trim();

            match field.as_str() {
                "user-agent" => {
                    if seen_directive_in_group && !current.agents.is_empty() {
                        groups.push(current);
                        current = AgentGroup::default();
                        seen_directive_in_group = false;
                    }
                    if !value.is_empty() {
                        current.agents.push(value.to_ascii_lowercase());
                    }
                }
                "disallow" => {
                    seen_directive_in_group = true;
                    // Per RFC 9309, an empty `Disallow:` grants access to everything.
                    if !value.is_empty() {
                        current.rules.push(RobotsRule {
                            kind: RuleKind::Disallow,
                            pattern: value.to_string(),
                        });
                    }
                }
                "allow" => {
                    seen_directive_in_group = true;
                    if !value.is_empty() {
                        current.rules.push(RobotsRule {
                            kind: RuleKind::Allow,
                            pattern: value.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }

        if !current.agents.is_empty() {
            groups.push(current);
        }

        Self { groups }
    }

    pub fn allows(&self, user_agent: &str, path: &str) -> bool {
        let ua_lower = user_agent.trim().to_ascii_lowercase();
        let ua_product = ua_lower
            .split(|c: char| c == '/' || c.is_whitespace())
            .next()
            .unwrap_or("");

        // Prefer the most specific matching user-agent group(s); fall back to `*`.
        let mut best_agent_len = 0usize;
        let mut specific_rules: Vec<&RobotsRule> = Vec::new();
        let mut wildcard_rules: Vec<&RobotsRule> = Vec::new();

        for group in &self.groups {
            for agent in &group.agents {
                if agent == "*" {
                    wildcard_rules.extend(group.rules.iter());
                    continue;
                }
                let matches = (!ua_product.is_empty() && ua_product == agent)
                    || (!ua_lower.is_empty() && ua_lower.starts_with(agent.as_str()));
                if matches {
                    if agent.len() > best_agent_len {
                        best_agent_len = agent.len();
                        specific_rules.clear();
                        specific_rules.extend(group.rules.iter());
                    } else if agent.len() == best_agent_len {
                        specific_rules.extend(group.rules.iter());
                    }
                }
            }
        }

        let active_rules = if best_agent_len > 0 {
            specific_rules
        } else {
            wildcard_rules
        };

        let normalized_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };

        let mut best_match: Option<(usize, &RuleKind)> = None;
        for rule in active_rules {
            if robots_pattern_matches(&rule.pattern, &normalized_path) {
                let len = rule.pattern.len();
                best_match = match best_match {
                    None => Some((len, &rule.kind)),
                    Some((prev_len, _)) if len > prev_len => Some((len, &rule.kind)),
                    Some((prev_len, prev_kind))
                        if len == prev_len && *prev_kind == RuleKind::Disallow =>
                    {
                        // RFC 9309: Allow wins over Disallow on equal pattern length.
                        Some((len, &rule.kind))
                    }
                    other => other,
                };
            }
        }

        !matches!(best_match, Some((_, RuleKind::Disallow)))
    }
}

/// Parses `robots_txt` and checks whether `user_agent` is permitted to fetch `path`.
pub fn parse_robots_allows(robots_txt: &str, user_agent: &str, path: &str) -> bool {
    RobotsPolicy::parse(robots_txt).allows(user_agent, path)
}

fn robots_pattern_matches(pattern: &str, path: &str) -> bool {
    let anchored_end = pattern.ends_with('$');
    let body = if anchored_end {
        &pattern[..pattern.len() - 1]
    } else {
        pattern
    };

    let parts: Vec<&str> = body.split('*').collect();
    if parts.is_empty() {
        return true;
    }

    // First segment must match from the start of the path.
    let Some(first) = parts.first() else {
        return true;
    };
    if !path.starts_with(first) {
        return false;
    }
    let mut cursor = first.len();

    for segment in parts.iter().skip(1) {
        if segment.is_empty() {
            continue;
        }
        let Some(offset) = path[cursor..].find(segment) else {
            return false;
        };
        cursor += offset + segment.len();
    }

    if anchored_end {
        // When trailing `*$` occurs, `parts.last()` is empty and matches to end.
        if body.ends_with('*') {
            true
        } else {
            cursor == path.len()
        }
    } else {
        true
    }
}

// ---------------------------------------------------------------------------
// BOM HTML Scraper Adapter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BomHtmlScraperSource {
    base_url: String,
    enabled: bool,
}

impl BomHtmlScraperSource {
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_BOM_HTML_BASE.to_string(),
            enabled: true,
        }
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            enabled: true,
        }
    }

    pub fn disabled() -> Self {
        Self {
            base_url: DEFAULT_BOM_HTML_BASE.to_string(),
            enabled: false,
        }
    }

    async fn fetch_robots_policy(
        &self,
        client: &reqwest::Client,
    ) -> Result<Option<RobotsPolicy>, SourceError> {
        let robots_url = format!("{}/robots.txt", self.base_url);
        let resp = match client.get(&robots_url).send().await {
            Ok(resp) => resp,
            Err(err) => {
                warn!(
                    source = self.source_id(),
                    error = %err,
                    "failed fetching robots.txt; aborting scrape for compliance"
                );
                return Ok(None);
            }
        };

        match resp.status() {
            StatusCode::NOT_FOUND => Ok(Some(RobotsPolicy::default())),
            status if status.is_success() => {
                let text = resp.text().await.map_err(SourceError::Http)?;
                Ok(Some(RobotsPolicy::parse(&text)))
            }
            status => {
                warn!(
                    source = self.source_id(),
                    status = %status,
                    "robots.txt returned non-success status; aborting scrape for compliance"
                );
                Ok(None)
            }
        }
    }
}

impl Default for BomHtmlScraperSource {
    fn default() -> Self {
        Self::new()
    }
}

fn default_bom_product_for_state(state: &StateCode) -> &'static str {
    match state {
        StateCode::NSW | StateCode::ACT => "IDN60901",
        StateCode::VIC => "IDV60901",
        StateCode::QLD => "IDQ60901",
        StateCode::SA => "IDS60901",
        StateCode::WA => "IDW60901",
        StateCode::TAS => "IDT60901",
        StateCode::NT => "IDD60901",
    }
}

#[async_trait]
impl WeatherSource for BomHtmlScraperSource {
    fn source_id(&self) -> &'static str {
        "bom_html_scraper"
    }

    fn display_name(&self) -> &'static str {
        "BOM HTML Scraper"
    }

    fn default_weight(&self) -> f64 {
        0.75
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
            .timeout(Duration::from_secs(ctx.timeout_secs.max(1)))
            .build()
            .map_err(SourceError::Http)?;

        let Some(policy) = self.fetch_robots_policy(&client).await? else {
            return Ok(SourceOutput::empty());
        };

        // Group nodes by BOM state observation product so each state page is fetched once.
        let products: BTreeSet<String> = nodes
            .iter()
            .map(|n| {
                n.bom_product_id
                    .clone()
                    .unwrap_or_else(|| default_bom_product_for_state(&n.state).to_string())
            })
            .collect();

        let mut combined = SourceOutput::empty();
        for product in products {
            let path = format!("/products/{product}/{product}.shtml");
            if !policy.allows(&ctx.user_agent, &path) {
                info!(
                    source = self.source_id(),
                    path = %path,
                    "robots.txt disallows scraping path; skipping"
                );
                continue;
            }

            let url = format!("{}{}", self.base_url, path);
            let resp = match client.get(&url).send().await {
                Ok(r) => r,
                Err(err) => {
                    warn!(source = self.source_id(), url = %url, error = %err, "HTML fetch failed");
                    continue;
                }
            };

            if !resp.status().is_success() {
                warn!(
                    source = self.source_id(),
                    url = %url,
                    status = %resp.status(),
                    "HTML fetch returned non-success status"
                );
                continue;
            }

            let html = match resp.text().await {
                Ok(t) => t,
                Err(err) => {
                    warn!(source = self.source_id(), url = %url, error = %err, "failed reading HTML");
                    continue;
                }
            };

            let fetched_at = Utc::now();
            let mut parsed = parse_bom_obs_html(&html, nodes, fetched_at);
            for (_, report) in &mut parsed.reports {
                if report.source_url.is_none() {
                    report.source_url = Some(url.clone());
                }
            }
            combined.reports.extend(parsed.reports);
            combined.observations.extend(parsed.observations);
        }

        Ok(combined)
    }
}

#[derive(Debug, Default)]
struct ColumnMap {
    station: Option<usize>,
    temp_c: Option<usize>,
    feels_like_c: Option<usize>,
    humidity_pct: Option<usize>,
    wind_kmh: Option<usize>,
    rain_mm: Option<usize>,
}

fn cell_text(el: ElementRef<'_>) -> String {
    el.text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_colspan(el: ElementRef<'_>) -> usize {
    el.value()
        .attr("colspan")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(1)
}

fn parse_rowspan(el: ElementRef<'_>) -> usize {
    el.value()
        .attr("rowspan")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(1)
}

/// Expands multi-row `<thead>` headers (accounting for `rowspan`/`colspan`) into
/// combined column titles per physical column index.
fn build_column_map(header_rows: &[ElementRef<'_>], cell_sel: &Selector) -> ColumnMap {
    let mut grid: Vec<Vec<Option<String>>> = Vec::new();

    for (row_idx, row) in header_rows.iter().enumerate() {
        while grid.len() <= row_idx {
            grid.push(Vec::new());
        }

        let mut col_idx = 0usize;
        for cell in row.select(cell_sel) {
            while grid[row_idx]
                .get(col_idx)
                .and_then(|v| v.as_ref())
                .is_some()
            {
                col_idx += 1;
            }
            let colspan = parse_colspan(cell);
            let rowspan = parse_rowspan(cell);
            let text = cell_text(cell);

            for r in row_idx..(row_idx + rowspan) {
                while grid.len() <= r {
                    grid.push(Vec::new());
                }
                for c in col_idx..(col_idx + colspan) {
                    while grid[r].len() <= c {
                        grid[r].push(None);
                    }
                    grid[r][c] = Some(text.clone());
                }
            }
            col_idx += colspan;
        }
    }

    let num_cols = grid.iter().map(Vec::len).max().unwrap_or(0);
    let mut map = ColumnMap::default();

    for col in 0..num_cols {
        let mut combined_parts = Vec::new();
        for row in &grid {
            if let Some(Some(txt)) = row.get(col) {
                let lower = txt.to_ascii_lowercase();
                if !lower.is_empty() && combined_parts.last() != Some(&lower) {
                    combined_parts.push(lower);
                }
            }
        }
        let header = combined_parts.join(" ");
        if header.is_empty() {
            continue;
        }

        if map.station.is_none()
            && (header.contains("station")
                || header.contains("location")
                || header.contains("name")
                || header.contains("site"))
        {
            map.station = Some(col);
        } else if map.feels_like_c.is_none()
            && (header.contains("app temp")
                || header.contains("app. temp")
                || header.contains("apparent")
                || header.contains("feels"))
        {
            map.feels_like_c = Some(col);
        } else if map.temp_c.is_none()
            && header.contains("temp")
            && !header.contains("dew")
            && !header.contains("app")
            && !header.contains("feels")
        {
            map.temp_c = Some(col);
        } else if map.humidity_pct.is_none()
            && (header.contains("hum") || header == "rh" || header.contains("rh %"))
        {
            map.humidity_pct = Some(col);
        } else if map.wind_kmh.is_none()
            && header.contains("wind")
            && !header.contains("dir")
            && !header.contains("gust")
            && !header.contains("kts")
            && !header.contains("kt")
            && (header.contains("spd")
                || header.contains("speed")
                || header.contains("km/h")
                || header.contains("kmh"))
        {
            map.wind_kmh = Some(col);
        } else if map.rain_mm.is_none() && header.contains("rain") {
            map.rain_mm = Some(col);
        }
    }

    map
}

fn parse_numeric_cell(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "-" || trimmed == "—" || trimmed.eq_ignore_ascii_case("n/a")
    {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("calm") || trimmed.eq_ignore_ascii_case("trace") {
        return Some(0.0);
    }

    let cleaned: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();
    if cleaned.is_empty() || cleaned == "-" || cleaned == "+" {
        return None;
    }
    cleaned.parse::<f64>().ok()
}

fn normalize_station_name(name: &str) -> String {
    name.split('(')
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_lowercase()
}

fn match_node_for_row<'a>(
    station_name: &str,
    row_hrefs: &[String],
    nodes: &'a [MeshNode],
) -> Option<&'a MeshNode> {
    let station_lower = station_name.trim().to_ascii_lowercase();
    let station_base = normalize_station_name(station_name);

    // 1. Match by BOM station ID embedded in anchor href (e.g. `IDN60901.94768.shtml`).
    for node in nodes {
        if let Some(station_id) = &node.bom_station_id {
            if row_hrefs.iter().any(|href| href.contains(station_id)) {
                return Some(node);
            }
        }
    }

    // 2. Exact case-insensitive match on full name.
    if let Some(node) = nodes
        .iter()
        .find(|n| n.name.trim().to_ascii_lowercase() == station_lower)
    {
        return Some(node);
    }

    // 3. Exact match after stripping parenthetical qualifier (e.g. "Sydney (Observatory Hill)").
    if let Some(node) = nodes
        .iter()
        .find(|n| normalize_station_name(&n.name) == station_base)
    {
        return Some(node);
    }

    // 4. Fallback substring match when unambiguous.
    let candidates: Vec<&MeshNode> = nodes
        .iter()
        .filter(|n| {
            let node_base = normalize_station_name(&n.name);
            !node_base.is_empty()
                && (station_base.contains(&node_base) || node_base.contains(&station_base))
        })
        .collect();
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }

    None
}

/// Pure deterministic parser for BOM HTML observation summary tables
/// (`/products/{product}/{product}.shtml`).
pub fn parse_bom_obs_html(
    html: &str,
    nodes: &[MeshNode],
    fetched_at: DateTime<Utc>,
) -> SourceOutput {
    if nodes.is_empty() {
        return SourceOutput::empty();
    }

    let doc = Html::parse_document(html);
    let table_sel = Selector::parse("table").expect("valid table selector");
    let thead_tr_sel = Selector::parse("thead tr").expect("valid thead tr selector");
    let tbody_tr_sel = Selector::parse("tbody tr").expect("valid tbody tr selector");
    let tr_sel = Selector::parse("tr").expect("valid tr selector");
    let cell_sel = Selector::parse("th, td").expect("valid cell selector");
    let a_sel = Selector::parse("a").expect("valid anchor selector");

    let mut seen_nodes: HashMap<String, usize> = HashMap::new();
    let mut output = SourceOutput::empty();

    for table in doc.select(&table_sel) {
        let mut header_rows: Vec<ElementRef<'_>> = table.select(&thead_tr_sel).collect();
        let data_rows: Vec<ElementRef<'_>> = if !header_rows.is_empty() {
            let tbody_rows: Vec<ElementRef<'_>> = table.select(&tbody_tr_sel).collect();
            if tbody_rows.is_empty() {
                table
                    .select(&tr_sel)
                    .filter(|tr| tr.select(&Selector::parse("td").unwrap()).next().is_some())
                    .collect()
            } else {
                tbody_rows
            }
        } else {
            let all_rows: Vec<ElementRef<'_>> = table.select(&tr_sel).collect();
            let td_sel = Selector::parse("td").expect("valid td selector");
            let first_data_idx = all_rows
                .iter()
                .position(|tr| tr.select(&td_sel).next().is_some())
                .unwrap_or(all_rows.len());
            header_rows = all_rows[..first_data_idx].to_vec();
            all_rows[first_data_idx..].to_vec()
        };

        if header_rows.is_empty() || data_rows.is_empty() {
            continue;
        }

        let col_map = build_column_map(&header_rows, &cell_sel);
        let station_col = col_map.station.unwrap_or(0);

        // Must have at least temperature or another weather column to treat as an obs table.
        if col_map.temp_c.is_none()
            && col_map.feels_like_c.is_none()
            && col_map.humidity_pct.is_none()
            && col_map.wind_kmh.is_none()
        {
            continue;
        }

        for row in data_rows {
            let cells: Vec<ElementRef<'_>> = row.select(&cell_sel).collect();
            let Some(station_cell) = cells.get(station_col) else {
                continue;
            };
            let station_name = cell_text(*station_cell);
            if station_name.is_empty() {
                continue;
            }

            let row_hrefs: Vec<String> = row
                .select(&a_sel)
                .filter_map(|a| a.value().attr("href").map(str::to_string))
                .collect();

            let Some(node) = match_node_for_row(&station_name, &row_hrefs, nodes) else {
                continue;
            };

            let read_col = |idx_opt: Option<usize>| -> Option<f64> {
                idx_opt
                    .and_then(|idx| cells.get(idx))
                    .and_then(|cell| parse_numeric_cell(&cell_text(*cell)))
            };

            let temp_c = read_col(col_map.temp_c);
            let feels_like_c = read_col(col_map.feels_like_c);
            let humidity_pct = read_col(col_map.humidity_pct).map(|h| h.clamp(0.0, 100.0));
            let wind_kmh = read_col(col_map.wind_kmh);
            let rain_mm = read_col(col_map.rain_mm);

            if temp_c.is_none() && feels_like_c.is_none() && humidity_pct.is_none() {
                continue;
            }

            let mut current = WeatherPoint {
                valid_at: fetched_at,
                temp_c,
                feels_like_c,
                feels_like_is_derived: false,
                rain_prob_pct: None,
                rain_mm,
                wind_kmh,
                humidity_pct,
                summary: Some(format!("BOM HTML table observation ({station_name})")),
            };
            ensure_feels_like(&mut current);

            let report = SourceReport {
                source_id: "bom_html_scraper".to_string(),
                display_name: "BOM HTML Scraper".to_string(),
                kind: SourceKind::Observation,
                fetched_at,
                weight: 0.75,
                current: current.clone(),
                hourly: Vec::new(),
                source_url: None,
            };

            let obs = ObservationRecord {
                node_id: node.node_id.clone(),
                observed_at: fetched_at,
                source_id: "bom_html_scraper".to_string(),
                quality_rank: BOM_SCRAPER_QUALITY_RANK,
                temp_c: current.temp_c,
                feels_like_c: current.feels_like_c,
                rain_mm_1h: current.rain_mm,
                rained_bool: current.rain_mm.map(|r| r > 0.0),
                wind_kmh: current.wind_kmh,
                humidity_pct: current.humidity_pct,
            };

            if let Some(&existing_idx) = seen_nodes.get(&node.node_id) {
                output.reports[existing_idx] = (node.node_id.clone(), report);
                output.observations[existing_idx] = obs;
            } else {
                seen_nodes.insert(node.node_id.clone(), output.reports.len());
                output.reports.push((node.node_id.clone(), report));
                output.observations.push(obs);
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robots_allows_wildcard_and_specific_agent_rules() {
        let robots = r#"
# Global rules
User-agent: *
Disallow: /cgi-bin/
Disallow: /products/
Allow: /products/IDN60901/

User-agent: TurtleHeavyWeather
Allow: /products/
Disallow: /places/private/
"#;

        assert!(parse_robots_allows(
            robots,
            "TurtleHeavyWeather/0.1 (+https://example.org)",
            "/products/IDV60901/IDV60901.shtml"
        ));
        assert!(!parse_robots_allows(
            robots,
            "TurtleHeavyWeather/0.1",
            "/places/private/secret.html"
        ));
        // Unrelated agent uses `*` rules: `/products/IDV60901/` disallowed, `/products/IDN60901/` allowed.
        assert!(!parse_robots_allows(
            robots,
            "OtherBot/1.0",
            "/products/IDV60901/IDV60901.shtml"
        ));
        assert!(parse_robots_allows(
            robots,
            "OtherBot/1.0",
            "/products/IDN60901/IDN60901.shtml"
        ));
    }

    #[test]
    fn robots_supports_wildcards_end_anchors_and_empty_disallow() {
        let robots = r#"
User-agent: *
Disallow: /*.json$
Allow: /public/*.json$
Disallow: /admin

User-agent: FriendlyBot
Disallow:
"#;

        assert!(!parse_robots_allows(robots, "SomeBot", "/data/feed.json"));
        assert!(parse_robots_allows(
            robots,
            "SomeBot",
            "/public/latest.json"
        ));
        assert!(parse_robots_allows(
            robots,
            "SomeBot",
            "/data/feed.json?v=1"
        ));
        assert!(!parse_robots_allows(robots, "SomeBot", "/admin/panel"));
        assert!(parse_robots_allows(
            robots,
            "FriendlyBot/2.0",
            "/admin/panel"
        ));
    }
}
