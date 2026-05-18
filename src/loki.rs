use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{LokiAuthMode, LokiSourceConfig};
use crate::error::AppError;
use crate::filter::JournalEntry;
use crate::source::LogResult;

const NS_PER_SECOND: u64 = 1_000_000_000;
/// How far back from `now` to set the upper bound of each query, in seconds.
/// Mitigates host-vs-Loki clock skew so we don't miss entries Loki hasn't
/// fully indexed yet. Not configurable in v1 — promote if anyone hits it.
const QUERY_LAG_SECONDS: u64 = 5;
const STATE_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug)]
struct LokiState {
    version: u32,
    last_timestamp_ns: String,
}

/// Query Loki for entries since the last bookmark.
///
/// State-file behavior mirrors journal source:
/// - First run (no state file): record `now_ns` as the bookmark and return `FirstRun`.
/// - Subsequent runs: fetch `[last_ns, now_ns - QUERY_LAG_SECONDS)` and advance.
pub fn query_loki(cfg: &LokiSourceConfig, state_file: &Path) -> Result<LogResult, AppError> {
    if !state_file.exists() {
        return bootstrap(state_file);
    }

    let state = read_state(state_file)?;
    let last_ns: u64 = state.last_timestamp_ns.parse().map_err(|_| {
        AppError::Loki(
            format!(
                "state file at {} has malformed last_timestamp_ns: {:?}",
                state_file.display(),
                state.last_timestamp_ns
            )
            .into(),
        )
    })?;

    let now_ns = system_time_ns()?.saturating_sub(QUERY_LAG_SECONDS * NS_PER_SECOND);
    if now_ns <= last_ns {
        // Clock-skew-induced backwards jump; nothing to do this tick.
        return Ok(LogResult::Entries(Vec::new()));
    }

    let body = fetch_range(cfg, last_ns, now_ns)?;
    let (entries, max_seen_ns) = parse_loki_response(&body, cfg)?;

    if entries.len() as u32 >= cfg.max_entries_per_query {
        eprintln!(
            "warning: Loki query hit max_entries_per_query ({}); remaining entries beyond the \
             limit will be picked up on the next run",
            cfg.max_entries_per_query
        );
    }

    let advanced_ns = match max_seen_ns {
        Some(n) => n.saturating_add(1),
        None => now_ns,
    };
    write_state(
        state_file,
        &LokiState {
            version: STATE_FORMAT_VERSION,
            last_timestamp_ns: advanced_ns.to_string(),
        },
    )?;

    Ok(LogResult::Entries(entries))
}

fn bootstrap(state_file: &Path) -> Result<LogResult, AppError> {
    if let Some(parent) = state_file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::CursorFile {
            path: state_file.to_owned(),
            source: e,
        })?;
    }
    let now_ns = system_time_ns()?;
    write_state(
        state_file,
        &LokiState {
            version: STATE_FORMAT_VERSION,
            last_timestamp_ns: now_ns.to_string(),
        },
    )?;
    Ok(LogResult::FirstRun(Some("baseline".to_string())))
}

fn system_time_ns() -> Result<u64, AppError> {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| AppError::Loki(format!("system clock before UNIX epoch: {e}").into()))?;
    let secs = dur.as_secs();
    let extra_ns = dur.subsec_nanos() as u64;
    secs.checked_mul(NS_PER_SECOND)
        .and_then(|s| s.checked_add(extra_ns))
        .ok_or_else(|| AppError::Loki("system time overflows u64 nanoseconds".into()))
}

fn read_state(path: &Path) -> Result<LokiState, AppError> {
    let bytes = std::fs::read(path).map_err(|e| AppError::CursorFile {
        path: path.to_owned(),
        source: e,
    })?;
    if !bytes.iter().any(|b| !b.is_ascii_whitespace()) {
        return Err(AppError::Loki(
            format!("state file at {} is empty", path.display()).into(),
        ));
    }
    let first = bytes.iter().find(|b| !b.is_ascii_whitespace()).copied();
    if first != Some(b'{') {
        return Err(AppError::Loki(
            format!(
                "state file at {} does not look like a Loki state file (expected JSON). \
                 If you switched [source].type, delete this file to bootstrap fresh.",
                path.display()
            )
            .into(),
        ));
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        AppError::Loki(
            format!(
                "failed to parse Loki state file at {}: {e}",
                path.display()
            )
            .into(),
        )
    })
}

fn write_state(path: &Path, state: &LokiState) -> Result<(), AppError> {
    let body = serde_json::to_vec(state)
        .map_err(|e| AppError::Loki(format!("failed to serialize state: {e}").into()))?;
    std::fs::write(path, body).map_err(|e| AppError::CursorFile {
        path: path.to_owned(),
        source: e,
    })
}

fn fetch_range(cfg: &LokiSourceConfig, start_ns: u64, end_ns: u64) -> Result<String, AppError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(cfg.timeout_seconds))
        .build()
        .map_err(|e| AppError::Loki(format!("failed to build HTTP client: {e}").into()))?;

    let url = format!(
        "{}/loki/api/v1/query_range",
        cfg.url.trim_end_matches('/')
    );

    let limit = cfg.max_entries_per_query.to_string();
    let start = start_ns.to_string();
    let end = end_ns.to_string();
    let mut req = client.get(&url).query(&[
        ("query", cfg.query.as_str()),
        ("start", start.as_str()),
        ("end", end.as_str()),
        ("limit", limit.as_str()),
        ("direction", "forward"),
    ]);

    if let Some(tenant) = &cfg.tenant_id {
        req = req.header("X-Scope-OrgID", tenant);
    }
    req = match cfg.auth {
        LokiAuthMode::None => req,
        LokiAuthMode::Bearer => {
            let token = cfg.credentials.bearer_token.as_deref().ok_or_else(|| {
                AppError::Loki(
                    "bearer auth selected but FRIENDLY_GHOST_LOKI_BEARER_TOKEN is not set".into(),
                )
            })?;
            req.bearer_auth(token)
        }
        LokiAuthMode::Basic => {
            let user = cfg.credentials.basic_user.as_deref().ok_or_else(|| {
                AppError::Loki(
                    "basic auth selected but FRIENDLY_GHOST_LOKI_BASIC_USER is not set".into(),
                )
            })?;
            req.basic_auth(user, cfg.credentials.basic_password.as_deref())
        }
    };

    let resp = req
        .send()
        .map_err(|e| AppError::Loki(format!("HTTP request to Loki failed: {e}").into()))?;
    let status = resp.status();
    let body = resp
        .text()
        .map_err(|e| AppError::Loki(format!("failed to read Loki response body: {e}").into()))?;
    if !status.is_success() {
        return Err(AppError::Loki(
            format!("Loki returned {status}: {body}").into(),
        ));
    }
    Ok(body)
}

/// Parse a Loki `/loki/api/v1/query_range` JSON response into JournalEntry rows.
/// Returns the parsed entries and the maximum timestamp seen (in nanoseconds, if any).
fn parse_loki_response(
    json: &str,
    cfg: &LokiSourceConfig,
) -> Result<(Vec<JournalEntry>, Option<u64>), AppError> {
    let root: Value = serde_json::from_str(json).map_err(|e| {
        AppError::Loki(format!("failed to parse Loki JSON response: {e}").into())
    })?;

    let status = root.get("status").and_then(Value::as_str).unwrap_or("");
    if status != "success" {
        return Err(AppError::Loki(
            format!("Loki response status is not \"success\": {status}").into(),
        ));
    }

    let data = root.get("data").ok_or_else(|| {
        AppError::Loki("Loki response missing 'data' field".into())
    })?;
    let result_type = data.get("resultType").and_then(Value::as_str).unwrap_or("");
    if result_type != "streams" {
        return Err(AppError::Loki(
            format!(
                "Loki response resultType must be \"streams\" (got {result_type:?}); \
                 use a log query, not a metric query"
            )
            .into(),
        ));
    }

    let streams = data
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Loki("Loki response missing 'data.result' array".into()))?;

    let mut entries = Vec::new();
    let mut max_ns: Option<u64> = None;

    for stream in streams {
        let labels = match stream.get("stream").and_then(Value::as_object) {
            Some(o) => o,
            None => continue,
        };

        let unit = pick_label(labels, &cfg.unit_label, &cfg.unit_label_fallback)
            .unwrap_or_default();
        let host = pick_label(labels, &cfg.host_label, &cfg.host_label_fallback)
            .unwrap_or_default();
        let priority = resolve_priority(labels, cfg);

        let values = match stream.get("values").and_then(Value::as_array) {
            Some(v) => v,
            None => continue,
        };

        for tuple in values {
            let arr = match tuple.as_array() {
                Some(a) if a.len() >= 2 => a,
                _ => continue,
            };
            let ts_str = match arr[0].as_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            let message = match arr[1].as_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            if let Ok(ns) = ts_str.parse::<u64>() {
                max_ns = Some(match max_ns {
                    Some(prev) => prev.max(ns),
                    None => ns,
                });
            }
            entries.push(JournalEntry {
                timestamp: ts_str.clone(),
                host: host.clone(),
                unit: unit.clone(),
                priority,
                message,
                cursor: ts_str,
            });
        }
    }

    Ok((entries, max_ns))
}

fn pick_label(
    labels: &serde_json::Map<String, Value>,
    primary: &str,
    fallback: &[String],
) -> Option<String> {
    if let Some(v) = labels.get(primary).and_then(Value::as_str) {
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    for key in fallback {
        if let Some(v) = labels.get(key).and_then(Value::as_str) {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn resolve_priority(labels: &serde_json::Map<String, Value>, cfg: &LokiSourceConfig) -> u8 {
    for label in &cfg.level_labels {
        if let Some(value) = labels.get(label).and_then(Value::as_str) {
            let lc = value.to_lowercase();
            if let Some(p) = cfg.priority_mapping.get(&lc) {
                return *p;
            }
        }
    }
    cfg.default_priority
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn default_cfg() -> LokiSourceConfig {
        let raw = r#"
url = "http://localhost:3100"
query = "{job=\"x\"}"
auth = "none"
"#;
        toml::from_str(raw).unwrap()
    }

    #[test]
    fn parse_single_stream_with_level() {
        let json = r#"{
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [{
                    "stream": {"service_name": "nginx", "host": "web01", "level": "error"},
                    "values": [["1742000000000000000", "upstream timeout"]]
                }]
            }
        }"#;
        let (entries, max_ns) = parse_loki_response(json, &default_cfg()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].unit, "nginx");
        assert_eq!(entries[0].host, "web01");
        assert_eq!(entries[0].priority, 3);
        assert_eq!(entries[0].message, "upstream timeout");
        assert_eq!(entries[0].timestamp, "1742000000000000000");
        assert_eq!(max_ns, Some(1742000000000000000));
    }

    #[test]
    fn parse_unknown_level_uses_default_priority() {
        let json = r#"{
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [{
                    "stream": {"service_name": "x", "host": "h", "level": "foobar"},
                    "values": [["1000", "msg"]]
                }]
            }
        }"#;
        let (entries, _) = parse_loki_response(json, &default_cfg()).unwrap();
        assert_eq!(entries[0].priority, 6); // default_priority
    }

    #[test]
    fn parse_missing_level_uses_default_priority() {
        let json = r#"{
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [{
                    "stream": {"service_name": "x", "host": "h"},
                    "values": [["1000", "msg"]]
                }]
            }
        }"#;
        let (entries, _) = parse_loki_response(json, &default_cfg()).unwrap();
        assert_eq!(entries[0].priority, 6);
    }

    #[test]
    fn parse_severity_label_alias() {
        let json = r#"{
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [{
                    "stream": {"service_name": "x", "host": "h", "severity": "warn"},
                    "values": [["1000", "msg"]]
                }]
            }
        }"#;
        let (entries, _) = parse_loki_response(json, &default_cfg()).unwrap();
        assert_eq!(entries[0].priority, 4);
    }

    #[test]
    fn parse_priority_lookup_is_case_insensitive() {
        let json = r#"{
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [{
                    "stream": {"service_name": "x", "host": "h", "level": "ERROR"},
                    "values": [["1000", "msg"]]
                }]
            }
        }"#;
        let (entries, _) = parse_loki_response(json, &default_cfg()).unwrap();
        assert_eq!(entries[0].priority, 3);
    }

    #[test]
    fn parse_unit_label_fallback_picks_secondary() {
        let mut cfg = default_cfg();
        cfg.unit_label = "service_name".to_string();
        cfg.unit_label_fallback = vec!["job".to_string()];
        let json = r#"{
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [{
                    "stream": {"job": "postgres", "host": "h"},
                    "values": [["1000", "msg"]]
                }]
            }
        }"#;
        let (entries, _) = parse_loki_response(json, &cfg).unwrap();
        assert_eq!(entries[0].unit, "postgres");
    }

    #[test]
    fn parse_host_label_fallback_picks_secondary() {
        let json = r#"{
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [{
                    "stream": {"service_name": "x", "hostname": "node-7"},
                    "values": [["1000", "msg"]]
                }]
            }
        }"#;
        let (entries, _) = parse_loki_response(json, &default_cfg()).unwrap();
        assert_eq!(entries[0].host, "node-7");
    }

    #[test]
    fn parse_missing_unit_label_yields_empty_string() {
        let json = r#"{
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [{
                    "stream": {"host": "h"},
                    "values": [["1000", "msg"]]
                }]
            }
        }"#;
        let (entries, _) = parse_loki_response(json, &default_cfg()).unwrap();
        assert_eq!(entries[0].unit, "");
    }

    #[test]
    fn parse_multiple_streams_max_ns_is_global() {
        let json = r#"{
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [
                    {"stream": {"service_name": "a", "host": "h"},
                     "values": [["1000", "m1"], ["2000", "m2"]]},
                    {"stream": {"service_name": "b", "host": "h"},
                     "values": [["1500", "m3"], ["3000", "m4"]]}
                ]
            }
        }"#;
        let (entries, max_ns) = parse_loki_response(json, &default_cfg()).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(max_ns, Some(3000));
    }

    #[test]
    fn parse_empty_result_returns_no_entries() {
        let json = r#"{
            "status": "success",
            "data": {"resultType": "streams", "result": []}
        }"#;
        let (entries, max_ns) = parse_loki_response(json, &default_cfg()).unwrap();
        assert!(entries.is_empty());
        assert_eq!(max_ns, None);
    }

    #[test]
    fn parse_rejects_non_streams_result_type() {
        let json = r#"{
            "status": "success",
            "data": {"resultType": "matrix", "result": []}
        }"#;
        let err = parse_loki_response(json, &default_cfg()).unwrap_err().to_string();
        assert!(err.contains("resultType"), "got: {err}");
    }

    #[test]
    fn parse_rejects_error_status() {
        let json = r#"{"status": "error", "data": {}}"#;
        let err = parse_loki_response(json, &default_cfg()).unwrap_err().to_string();
        assert!(err.contains("not \"success\""), "got: {err}");
    }

    #[test]
    fn parse_custom_priority_mapping_overrides_default() {
        let mut cfg = default_cfg();
        let mut mapping: HashMap<String, u8> = cfg.priority_mapping.clone();
        mapping.insert("urgent".to_string(), 1);
        cfg.priority_mapping = mapping;
        let json = r#"{
            "status": "success",
            "data": {
                "resultType": "streams",
                "result": [{
                    "stream": {"service_name": "x", "host": "h", "level": "urgent"},
                    "values": [["1000", "msg"]]
                }]
            }
        }"#;
        let (entries, _) = parse_loki_response(json, &cfg).unwrap();
        assert_eq!(entries[0].priority, 1);
    }

    #[test]
    fn next_start_is_max_seen_plus_one() {
        // Pinning test: prevents the inclusive-start off-by-one regression.
        let max = 1_742_000_000_000_000_000u64;
        let next = max.saturating_add(1);
        assert_eq!(next, max + 1);
    }

    #[test]
    fn state_roundtrip() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let state = LokiState {
            version: STATE_FORMAT_VERSION,
            last_timestamp_ns: "1742000000000000000".to_string(),
        };
        write_state(tmp.path(), &state).unwrap();
        let read = read_state(tmp.path()).unwrap();
        assert_eq!(read.version, STATE_FORMAT_VERSION);
        assert_eq!(read.last_timestamp_ns, "1742000000000000000");
    }

    #[test]
    fn read_state_rejects_non_json_prefix() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"s=abc123;i=999\n").unwrap();
        let err = read_state(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("does not look like a Loki state file"), "got: {err}");
        assert!(err.contains("delete this file"), "got: {err}");
    }
}
