//! Search query language parser (search.md §Query Language).
//!
//! Pure logic, no infrastructure: a raw query string in the user-facing query
//! language is parsed into a structured [`ParsedQuery`] — free-text terms,
//! quoted phrases, and facet filters — which the search adapters execute.
//!
//! Grammar (search.md §Query Language):
//! - **Simple search**: `identity security JWT` — space-separated terms,
//!   matched with AND semantics by the adapters.
//! - **Quoted phrases**: `"merge queue" ordering` — the quoted span is an
//!   exact phrase; the rest are terms.
//! - **Faceted filtering**: `identity type:spec status:approved` —
//!   `key:value` tokens filter by entity type and attributes. Facets compose
//!   with AND across keys; multiple values for the same key compose with OR.
//! - **Time facets**: `since:`/`until:` accept relative (`7d`, `24h`, `2w`)
//!   and absolute (`2026-03-01`) values, resolved to epoch-second bounds by
//!   [`ParsedQuery::time_bounds`].
//!
//! The parser is forgiving by contract: malformed input (unterminated quotes,
//! `type:` with no value, unrecognized facet keys, invalid time values) never
//! errors — the malformed token is ignored or falls back to a plain search
//! term.

use std::collections::HashMap;

/// Facet keys the query language recognizes (search.md §Faceted Filtering).
///
/// This list is the whitelist that decides which `key:value` tokens are
/// facet filters. Anything else is not a facet: the token falls back to
/// plain search terms, so no unvetted key ever reaches a storage backend's
/// facet-filter path.
pub const FACET_KEYS: [&str; 12] = [
    "type", "workspace", "repo", "status", "priority", "assigned", "author", "label", "approval",
    "scope", "since", "until",
];

/// Facet keys that filter the document timestamp rather than the metadata
/// facet map. Handled by [`ParsedQuery::time_bounds`], never by
/// [`ParsedQuery::facet_filters`].
const TIME_FACET_KEYS: [&str; 2] = ["since", "until"];

/// A parsed search query (search.md §Query Language).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    /// Free-text terms, matched with AND semantics. Each term is a single
    /// word (tokens are split on non-alphanumeric boundaries, mirroring the
    /// FTS `unicode61` tokenizer).
    pub terms: Vec<String>,
    /// Quoted phrases, matched as exact phrases (not individual terms).
    pub phrases: Vec<String>,
    /// Facet filters, keyed by facet name with their raw values. Multiple
    /// values for one key compose with OR; keys compose with AND.
    /// `since`/`until` values are raw — resolve them via
    /// [`ParsedQuery::time_bounds`].
    pub facets: HashMap<String, Vec<String>>,
}

/// One facet filter, keyed by where it applies.
///
/// Produced by [`ParsedQuery::facet_filters`]; the mapping to concrete
/// storage (columns vs. metadata JSON) is adapter concern, but the key set
/// is fixed here so no unvetted key is ever interpolated into a query path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FacetFilter {
    /// `type:` — the document's entity type.
    EntityType(Vec<String>),
    /// `workspace:` — the document's workspace scope.
    Workspace(Vec<String>),
    /// `repo:` — the document's repo scope.
    Repo(Vec<String>),
    /// Any other facet key — the document's metadata facet map.
    Metadata { key: String, values: Vec<String> },
}

/// Resolved `since:`/`until:` bounds as unix epoch seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimeBounds {
    /// Inclusive lower bound (`updated_at >= since`).
    pub since: Option<i64>,
    /// Inclusive upper bound (`updated_at <= until`).
    pub until: Option<i64>,
}

/// Parse a raw query string into a structured [`ParsedQuery`].
///
/// Forgiving by contract: malformed tokens never error (see module docs).
pub fn parse_query(raw: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let mut token = String::new();

    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // Quoted span: everything up to the next `"` is one phrase. An
            // unterminated quote runs to the end of the input.
            '"' => {
                if !token.is_empty() {
                    take_token(&mut parsed, &token);
                    token.clear();
                }
                let mut phrase = String::new();
                for c2 in chars.by_ref() {
                    if c2 == '"' {
                        break;
                    }
                    phrase.push(c2);
                }
                let phrase = phrase.trim().to_string();
                if !phrase.is_empty() {
                    parsed.phrases.push(phrase);
                }
            }
            c if c.is_whitespace() => {
                if !token.is_empty() {
                    take_token(&mut parsed, &token);
                    token.clear();
                }
            }
            c => token.push(c),
        }
    }
    if !token.is_empty() {
        take_token(&mut parsed, &token);
    }
    parsed
}

/// Classify one unquoted whitespace-delimited token: recognized facet,
/// ignored malformed facet, or plain search term(s).
fn take_token(parsed: &mut ParsedQuery, token: &str) {
    if let Some((key, value)) = token.split_once(':') {
        let key = key.to_lowercase();
        if FACET_KEYS.contains(&key.as_str()) {
            if value.is_empty() {
                // Malformed facet (`type:` with no value) — ignored entirely,
                // not turned into a search term.
                return;
            }
            parsed
                .facets
                .entry(key)
                .or_default()
                .push(value.to_string());
            return;
        }
    }
    // Plain term: split on non-alphanumeric characters — the same token
    // boundaries the FTS `unicode61` tokenizer applies on the index side —
    // so each term is a single literal token.
    for term in token.split(|c: char| !c.is_alphanumeric()) {
        if !term.is_empty() {
            parsed.terms.push(term.to_string());
        }
    }
}

impl ParsedQuery {
    /// Whether any text (term or phrase) must match. A query with no text
    /// and no filters matches nothing; a facet-only query is valid.
    pub fn has_text(&self) -> bool {
        !self.terms.is_empty() || !self.phrases.is_empty()
    }

    /// Whether any facet filter (including time facets) applies — i.e. the
    /// query is more than free text.
    pub fn has_filters(&self) -> bool {
        !self.facets.is_empty()
    }

    /// Facet filters in a fixed key order, excluding `since:`/`until:` (time
    /// bounds — see [`ParsedQuery::time_bounds`]).
    pub fn facet_filters(&self) -> Vec<FacetFilter> {
        let mut out = Vec::new();
        for key in FACET_KEYS {
            if TIME_FACET_KEYS.contains(&key) {
                continue;
            }
            if let Some(values) = self.facets.get(key) {
                if values.is_empty() {
                    continue;
                }
                let filter = match key {
                    "type" => FacetFilter::EntityType(values.clone()),
                    "workspace" => FacetFilter::Workspace(values.clone()),
                    "repo" => FacetFilter::Repo(values.clone()),
                    _ => FacetFilter::Metadata {
                        key: key.to_string(),
                        values: values.clone(),
                    },
                };
                out.push(filter);
            }
        }
        out
    }

    /// Resolve the `since:`/`until:` facets to inclusive epoch-second bounds,
    /// relative to `now`. Relative values (`7d`) are `now - offset`; an
    /// absolute `since:2026-03-01` is the start of that UTC day and an
    /// absolute `until:2026-03-20` is the end of it. The first value of each
    /// facet wins (OR-composing instants is meaningless); an unparseable
    /// value is ignored — no bound.
    pub fn time_bounds(&self, now: i64) -> TimeBounds {
        TimeBounds {
            since: self
                .facets
                .get("since")
                .and_then(|vs| vs.first())
                .and_then(|v| parse_since(v, now)),
            until: self
                .facets
                .get("until")
                .and_then(|vs| vs.first())
                .and_then(|v| parse_until(v, now)),
        }
    }
}

/// A parsed time-facet value.
enum TimeValue {
    /// Offset backwards from now, in seconds (`7d`, `24h`, `2w`).
    Relative(i64),
    /// Start of a UTC calendar day, in epoch seconds (`2026-03-01`).
    Absolute(i64),
}

/// Parse a `since:`/`until:` value: relative (`7d`/`24h`/`2w`) or absolute
/// (`YYYY-MM-DD`). Anything else is `None` (facet ignored).
fn parse_time_value(raw: &str) -> Option<TimeValue> {
    // Relative: digits followed by a single unit character (`7d`, `24h`, `2w`).
    // A date like `2026-03-01` also starts with a digit, so the unit check and
    // integer parse must both succeed before it is treated as relative.
    if let Some(unit) = raw.chars().last() {
        if unit.is_ascii_alphabetic() {
            let unit_secs = match unit {
                'd' | 'D' => 86_400,
                'h' | 'H' => 3_600,
                'w' | 'W' => 604_800,
                _ => 0,
            };
            if unit_secs > 0 {
                if let Ok(value) = raw[..raw.len() - unit.len_utf8()].parse::<i64>() {
                    return Some(TimeValue::Relative(value.checked_mul(unit_secs)?));
                }
            }
        }
    }
    // Absolute: YYYY-MM-DD.
    parse_absolute_day(raw).map(TimeValue::Absolute)
}

fn parse_since(raw: &str, now: i64) -> Option<i64> {
    match parse_time_value(raw)? {
        TimeValue::Relative(secs) => now.checked_sub(secs),
        TimeValue::Absolute(day_start) => Some(day_start),
    }
}

fn parse_until(raw: &str, now: i64) -> Option<i64> {
    match parse_time_value(raw)? {
        TimeValue::Relative(secs) => now.checked_sub(secs),
        // Inclusive end of the calendar day: 23:59:59 UTC.
        TimeValue::Absolute(day_start) => day_start.checked_add(86_399),
    }
}

/// Parse `YYYY-MM-DD` into the epoch second of that UTC day's start.
/// Validates month and day-of-month (including leap years).
fn parse_absolute_day(raw: &str) -> Option<i64> {
    let mut parts = raw.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    if !(1..=12).contains(&month) {
        return None;
    }
    if day < 1 || day > days_in_month(year, month) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Days since 1970-01-01 for a civil (proleptic Gregorian) date — Howard
/// Hinnant's `days_from_civil`, valid for any representable date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400; // [0, 399]
    let month_prime = (month + 9) % 12; // [0, 11] (March = 0)
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1; // [0, 365]
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facets(pq: &ParsedQuery) -> String {
        // Deterministic rendering for assertions.
        let mut keys: Vec<&String> = pq.facets.keys().collect();
        keys.sort();
        keys.iter()
            .map(|k| format!("{k}={:?}", pq.facets[*k]))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn simple_terms() {
        let pq = parse_query("identity security JWT");
        assert_eq!(pq.terms, ["identity", "security", "JWT"]);
        assert!(pq.phrases.is_empty());
        assert!(pq.facets.is_empty());
        assert!(pq.has_text());
    }

    #[test]
    fn quoted_phrase_extraction() {
        let pq = parse_query("\"merge queue\" ordering");
        assert_eq!(pq.phrases, ["merge queue"]);
        assert_eq!(pq.terms, ["ordering"]);
    }

    #[test]
    fn phrase_then_term_then_phrase() {
        let pq = parse_query("alpha \"two words\" beta \"three more words\"");
        assert_eq!(pq.phrases, ["two words", "three more words"]);
        assert_eq!(pq.terms, ["alpha", "beta"]);
    }

    #[test]
    fn facet_extraction() {
        let pq = parse_query("identity type:spec status:approved workspace:platform-team");
        assert_eq!(pq.terms, ["identity"]);
        assert_eq!(pq.facets["type"], ["spec"]);
        assert_eq!(pq.facets["status"], ["approved"]);
        assert_eq!(pq.facets["workspace"], ["platform-team"]);
    }

    #[test]
    fn mixed_terms_phrase_facets() {
        let pq = parse_query("identity \"merge queue\" type:spec since:7d");
        assert_eq!(pq.terms, ["identity"]);
        assert_eq!(pq.phrases, ["merge queue"]);
        assert_eq!(pq.facets["type"], ["spec"]);
        assert_eq!(pq.facets["since"], ["7d"]);
    }

    #[test]
    fn multiple_values_same_facet_compose_or() {
        let pq = parse_query("type:spec type:task");
        assert_eq!(pq.facets["type"], ["spec", "task"]);
        assert!(pq.terms.is_empty());
    }

    #[test]
    fn facet_value_with_colon_and_at() {
        // `assigned:agent:worker-42` splits at the FIRST colon only.
        let pq = parse_query("assigned:agent:worker-42 author:@jsell");
        assert_eq!(pq.facets["assigned"], ["agent:worker-42"]);
        assert_eq!(pq.facets["author"], ["@jsell"]);
    }

    #[test]
    fn malformed_facet_with_no_value_is_ignored() {
        let pq = parse_query("identity type: status:");
        // `type:` and `status:` are recognized keys with empty values —
        // dropped entirely, not turned into terms.
        assert_eq!(pq.terms, ["identity"]);
        assert!(pq.facets.is_empty());
    }

    #[test]
    fn unknown_facet_key_falls_back_to_terms() {
        let pq = parse_query("frobnicate:widget");
        assert_eq!(pq.terms, ["frobnicate", "widget"]);
        assert!(pq.facets.is_empty());
    }

    #[test]
    fn terms_split_on_non_alphanumeric_boundaries() {
        // Mirrors the unicode61 tokenizer: `JWT-based` is two tokens.
        let pq = parse_query("JWT-based e2e.test");
        assert_eq!(pq.terms, ["JWT", "based", "e2e", "test"]);
    }

    #[test]
    fn unterminated_quote_runs_to_end() {
        let pq = parse_query("\"unterminated ( AND -^:*");
        assert_eq!(pq.phrases, ["unterminated ( AND -^:*"]);
        assert!(pq.terms.is_empty());
    }

    #[test]
    fn empty_and_whitespace_queries() {
        assert_eq!(parse_query(""), ParsedQuery::default());
        assert_eq!(parse_query("   "), ParsedQuery::default());
        assert!(!parse_query("").has_text());
    }

    #[test]
    fn empty_quoted_span_is_dropped() {
        let pq = parse_query("\"\" term");
        assert!(pq.phrases.is_empty());
        assert_eq!(pq.terms, ["term"]);
    }

    #[test]
    fn facet_keys_are_case_insensitive() {
        let pq = parse_query("TYPE:spec Status:approved");
        assert_eq!(pq.facets["type"], ["spec"]);
        assert_eq!(pq.facets["status"], ["approved"]);
    }

    #[test]
    fn facet_filters_map_keys_to_storage_classes() {
        let pq = parse_query("type:spec type:task workspace:ws-a repo:gyre-server status:open priority:high since:7d until:2026-03-20");
        let filters = pq.facet_filters();
        assert_eq!(
            filters,
            vec![
                FacetFilter::EntityType(vec!["spec".into(), "task".into()]),
                FacetFilter::Workspace(vec!["ws-a".into()]),
                FacetFilter::Repo(vec!["gyre-server".into()]),
                FacetFilter::Metadata {
                    key: "status".into(),
                    values: vec!["open".into()]
                },
                FacetFilter::Metadata {
                    key: "priority".into(),
                    values: vec!["high".into()]
                },
                // since/until are time bounds, never metadata filters.
            ]
        );
    }

    #[test]
    fn has_filters_distinguishes_facet_only_queries() {
        assert!(parse_query("type:spec").has_filters());
        assert!(!parse_query("type:spec").has_text());
        // Free text alone is not a facet filter.
        assert!(!parse_query("term").has_filters());
        assert!(parse_query("term").has_text());
    }

    #[test]
    fn time_bounds_relative() {
        let now = 1_800_000_000;
        let pq = parse_query("since:7d until:24h");
        assert_eq!(pq.time_bounds(now).since, Some(now - 7 * 86_400));
        assert_eq!(pq.time_bounds(now).until, Some(now - 3_600 * 24));
    }

    #[test]
    fn time_bounds_relative_weeks() {
        let now = 1_800_000_000;
        let pq = parse_query("since:2w");
        assert_eq!(pq.time_bounds(now).since, Some(now - 14 * 86_400));
    }

    #[test]
    fn time_bounds_absolute() {
        let pq = parse_query("since:2026-03-01 until:2026-03-20");
        let tb = pq.time_bounds(0);
        // 2026-03-01T00:00:00Z = 1772323200.
        assert_eq!(tb.since, Some(1_772_323_200));
        // End of 2026-03-20 (23:59:59Z).
        assert_eq!(tb.until, Some(1_772_323_200 + 19 * 86_400 + 86_399));
    }

    #[test]
    fn time_bounds_absolute_leap_day() {
        let pq = parse_query("since:2024-02-29");
        // 2024-02-29T00:00:00Z = 1709164800.
        assert_eq!(pq.time_bounds(0).since, Some(1_709_164_800));
    }

    #[test]
    fn time_bounds_invalid_values_are_ignored() {
        for bad in ["7x", "banana", "2026-13-01", "2026-02-30", "2026-3", "7"] {
            let pq = parse_query(&format!("since:{bad}"));
            assert_eq!(pq.time_bounds(0).since, None, "since:{bad} must not parse");
        }
        // The raw value stays in the facets map; it just resolves to no bound.
        assert_eq!(parse_query("since:banana").facets["since"], ["banana"]);
    }

    #[test]
    fn time_bounds_first_value_wins() {
        let pq = parse_query("since:7d since:30d");
        assert_eq!(pq.time_bounds(1_800_000_000).since, Some(1_800_000_000 - 7 * 86_400));
    }

    #[test]
    fn time_bounds_absent_when_no_time_facets() {
        let pq = parse_query("identity type:spec");
        assert_eq!(pq.time_bounds(1_800_000_000), TimeBounds::default());
    }

    #[test]
    fn days_from_civil_epoch_reference() {
        // 1970-01-01 → 0; nearby dates round-trip through ±1 day.
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }

    #[test]
    fn facets_debug_rendering_helper_is_stable() {
        let pq = parse_query("status:open type:task");
        assert_eq!(facets(&pq), "status=[\"open\"] type=[\"task\"]");
    }
}
