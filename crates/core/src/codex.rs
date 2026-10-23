use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::model::{Source, TokenCounts, Tool, UsageEvent};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CodexScanStats {
    /// Matching rollout paths attempted, including paths that cannot be read.
    pub files: u64,
    /// Lines successfully read from matching paths, including malformed JSON.
    pub total_lines: u64,
    /// JSON decoding failures or usage/snapshot lines with invalid timestamps.
    pub malformed_lines: u64,
    /// Valid JSON lines that do not yield a selected usage event, excluding duplicates.
    pub non_usage_lines: u64,
    /// JSON token_usage_record lines, including invalid timestamps and absent or zero usage.
    pub usage_records: u64,
    /// Nonzero token_count usage lines in record-free files, before deduplication.
    pub token_count_usage_lines: u64,
    /// Repeated response IDs or adjacent identical cumulative token_count totals.
    pub deduplicated_lines: u64,
    /// Files containing any decoded token_usage_record line, even if unusable.
    pub files_with_records: u64,
    /// Record-free files containing a decoded token_count line.
    pub files_token_count_only: u64,
    /// Stored token_count snapshots with at least one rate-limit window.
    pub rate_limit_snapshots: u64,
    /// Matching paths that could not be opened or fully read.
    pub unreadable_files: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodexScan {
    pub events: Vec<UsageEvent>,
    pub rate_limits: Vec<RateLimitSnapshot>,
    pub stats: CodexScanStats,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RateLimitWindow {
    pub used_percent: f64,
    pub window_minutes: Option<u64>,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RateLimitSnapshot {
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub source: Source,
    pub limit_id: Option<String>,
    pub plan_type: Option<String>,
    pub primary: Option<RateLimitWindow>,
    pub secondary: Option<RateLimitWindow>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CodexDebugSummary {
    pub stats: CodexScanStats,
    pub events_total: usize,
    pub unique_sessions: usize,
    pub tokens: TokenCounts,
    pub sources: CodexSourceCounts,
    pub models: BTreeMap<String, CodexModelCounts>,
    pub rate_limit_snapshots: usize,
    pub latest_rate_limit: Option<LatestRateLimitSummary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LatestRateLimitSummary {
    pub limit_id: Option<String>,
    pub plan_type: Option<String>,
    pub primary: Option<RateLimitWindow>,
    pub secondary: Option<RateLimitWindow>,
}

impl From<&RateLimitSnapshot> for LatestRateLimitSummary {
    fn from(snapshot: &RateLimitSnapshot) -> Self {
        Self {
            limit_id: snapshot.limit_id.clone(),
            plan_type: snapshot.plan_type.clone(),
            primary: snapshot.primary.clone(),
            secondary: snapshot.secondary.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CodexSourceCounts {
    pub codex_cli: u64,
    pub codex_desktop: u64,
    pub unknown: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CodexModelCounts {
    pub events: u64,
    pub output: u64,
}

impl CodexScan {
    #[must_use]
    pub fn debug_summary(&self) -> CodexDebugSummary {
        let mut summary = CodexDebugSummary {
            stats: self.stats.clone(),
            events_total: self.events.len(),
            rate_limit_snapshots: self.rate_limits.len(),
            latest_rate_limit: self.rate_limits.last().map(LatestRateLimitSummary::from),
            ..CodexDebugSummary::default()
        };
        let mut sessions = HashSet::new();
        for event in &self.events {
            sessions.insert(&event.session_id);
            summary.tokens.add_saturating(&event.tokens);
            match event.source {
                Source::CodexCli => summary.sources.codex_cli += 1,
                Source::CodexDesktop => summary.sources.codex_desktop += 1,
                _ => summary.sources.unknown += 1,
            }
            let model = summary.models.entry(event.model.clone()).or_default();
            model.events += 1;
            model.output = model.output.saturating_add(event.tokens.output);
        }
        summary.unique_sessions = sessions.len();
        summary
    }
}

#[derive(Deserialize)]
struct RawLine {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    payload: Option<RawPayload>,
}

#[derive(Deserialize)]
struct RawPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    id: Option<String>,
    session_id: Option<String>,
    response_id: Option<String>,
    originator: Option<String>,
    thread_source: Option<String>,
    source: Option<SubagentSource>,
    cwd: Option<String>,
    model: Option<String>,
    usage: Option<RawUsage>,
    info: Option<RawInfo>,
    rate_limits: Option<RawRateLimits>,
}

#[derive(Default)]
struct SubagentSource(bool);

impl<'de> Deserialize<'de> for SubagentSource {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SourceVisitor;
        impl<'de> Visitor<'de> for SourceVisitor {
            type Value = SubagentSource;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a source object or scalar")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut subagent = false;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "subagent" {
                        subagent = true;
                    }
                    map.next_value::<IgnoredAny>()?;
                }
                Ok(SubagentSource(subagent))
            }

            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(SubagentSource(false))
            }

            fn visit_string<E: serde::de::Error>(self, _: String) -> Result<Self::Value, E> {
                Ok(SubagentSource(false))
            }

            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(SubagentSource(false))
            }

            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(SubagentSource(false))
            }

            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(SubagentSource(false))
            }

            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(SubagentSource(false))
            }

            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut sequence: S,
            ) -> Result<Self::Value, S::Error> {
                while sequence.next_element::<IgnoredAny>()?.is_some() {}
                Ok(SubagentSource(false))
            }
        }
        deserializer.deserialize_any(SourceVisitor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct RawUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    cache_write_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    reasoning_output_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
}

#[derive(Deserialize)]
struct RawInfo {
    total_token_usage: Option<RawUsage>,
    last_token_usage: Option<RawUsage>,
}

#[derive(Deserialize)]
struct RawRateLimits {
    limit_id: Option<String>,
    plan_type: Option<String>,
    primary: Option<RawRateLimitWindow>,
    secondary: Option<RawRateLimitWindow>,
}

#[derive(Deserialize)]
struct RawRateLimitWindow {
    used_percent: f64,
    window_minutes: Option<u64>,
    resets_at: Option<i64>,
}

impl RawRateLimitWindow {
    fn into_window(self) -> RateLimitWindow {
        RateLimitWindow {
            used_percent: self.used_percent,
            window_minutes: self.window_minutes,
            resets_at: self
                .resets_at
                .and_then(DateTime::<Utc>::from_timestamp_secs),
        }
    }
}

struct FileContext {
    session_id: String,
    source: Source,
    is_subagent: bool,
    project: Option<String>,
    model: String,
}

impl Default for FileContext {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            source: Source::Unknown,
            is_subagent: false,
            project: None,
            model: String::new(),
        }
    }
}

fn source_from_originator(originator: Option<&str>) -> Source {
    match originator {
        Some("codex-tui" | "codex_exec" | "codex_cli_rs") => Source::CodexCli,
        Some(value) if value.to_ascii_lowercase().contains("desktop") => Source::CodexDesktop,
        _ => Source::Unknown,
    }
}

fn filename_session_id(path: &Path) -> String {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return String::new();
    };
    let Some(uuid) = stem
        .strip_prefix("rollout-")
        .and_then(|value| value.get(value.len().saturating_sub(36)..))
    else {
        return String::new();
    };
    let bytes = uuid.as_bytes();
    if bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        uuid.to_owned()
    } else {
        String::new()
    }
}

fn tokens(usage: RawUsage) -> TokenCounts {
    TokenCounts {
        input: usage.input_tokens.saturating_sub(usage.cached_input_tokens),
        cache_read: usage.cached_input_tokens,
        cache_write_unspecified: usage.cache_write_input_tokens,
        output: usage.output_tokens,
        reasoning: usage.reasoning_output_tokens,
        cache_write_5m: 0,
        cache_write_1h: 0,
    }
}

fn event(
    timestamp: DateTime<Utc>,
    context: &FileContext,
    usage: RawUsage,
    session_id: Option<String>,
    request_id: Option<String>,
) -> Option<UsageEvent> {
    let tokens = tokens(usage);
    if tokens.is_zero() {
        return None;
    }
    Some(UsageEvent {
        tool: Tool::Codex,
        source: context.source,
        session_id: session_id
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| context.session_id.clone()),
        request_id,
        timestamp,
        model: context.model.clone(),
        project: context.project.clone(),
        is_subagent: context.is_subagent,
        tokens,
    })
}

fn timestamp(raw: &RawLine) -> Option<DateTime<Utc>> {
    raw.timestamp
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn scan_file(
    path: &Path,
    scan: &mut CodexScan,
    seen_responses: &mut HashSet<String>,
) -> io::Result<()> {
    let file = File::open(path)?;
    scan_reader(path, BufReader::new(file), scan, seen_responses)
}

fn scan_reader(
    path: &Path,
    mut reader: impl BufRead,
    scan: &mut CodexScan,
    seen_responses: &mut HashSet<String>,
) -> io::Result<()> {
    let mut line = Vec::new();
    let mut context = FileContext {
        session_id: filename_session_id(path),
        ..FileContext::default()
    };
    let mut records = Vec::new();
    let mut fallback = Vec::new();
    let mut rate_limits = Vec::new();
    let mut previous_total: Option<RawUsage> = None;
    let mut has_record = false;
    let mut has_token_count = false;
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => scan.stats.total_lines += 1,
            Err(error) => return Err(error),
        }
        let raw: RawLine = match serde_json::from_slice(&line) {
            Ok(raw) => raw,
            Err(_) => {
                scan.stats.malformed_lines += 1;
                continue;
            }
        };
        let parsed_timestamp = timestamp(&raw);
        let Some(payload) = raw.payload else {
            scan.stats.non_usage_lines += 1;
            continue;
        };
        match raw.kind.as_deref() {
            Some("session_meta") => {
                if let Some(id) = payload.id.filter(|id| !id.is_empty()) {
                    context.session_id = id;
                }
                context.source = source_from_originator(payload.originator.as_deref());
                context.is_subagent = payload.source.is_some_and(|source| source.0)
                    || payload.thread_source.as_deref() == Some("subagent");
                context.project = payload.cwd;
                scan.stats.non_usage_lines += 1;
            }
            Some("turn_context") => {
                if let Some(model) = payload.model {
                    context.model = model;
                }
                if let Some(cwd) = payload.cwd {
                    context.project = Some(cwd);
                }
                scan.stats.non_usage_lines += 1;
            }
            Some("token_usage_record") => {
                has_record = true;
                scan.stats.usage_records += 1;
                let Some(timestamp) = parsed_timestamp else {
                    scan.stats.malformed_lines += 1;
                    continue;
                };
                if let Some(usage) = payload.usage {
                    let request_id = payload.response_id.filter(|id| !id.is_empty());
                    match event(timestamp, &context, usage, payload.session_id, request_id) {
                        Some(event) => records.push(event),
                        None => scan.stats.non_usage_lines += 1,
                    }
                } else {
                    scan.stats.non_usage_lines += 1;
                }
            }
            Some("event_msg") if payload.kind.as_deref() == Some("token_count") => {
                has_token_count = true;
                let Some(timestamp) = parsed_timestamp else {
                    scan.stats.malformed_lines += 1;
                    continue;
                };
                if let Some(rate) = payload.rate_limits {
                    let primary = rate.primary.map(RawRateLimitWindow::into_window);
                    let secondary = rate.secondary.map(RawRateLimitWindow::into_window);
                    if primary.is_some() || secondary.is_some() {
                        rate_limits.push(RateLimitSnapshot {
                            timestamp,
                            session_id: context.session_id.clone(),
                            source: context.source,
                            limit_id: rate.limit_id,
                            plan_type: rate.plan_type,
                            primary,
                            secondary,
                        });
                    }
                }
                let info = payload.info;
                let duplicate = if let Some(total) = info
                    .as_ref()
                    .and_then(|info| info.total_token_usage.clone())
                {
                    let duplicate = previous_total.as_ref() == Some(&total);
                    previous_total = Some(total);
                    duplicate
                } else {
                    false
                };
                let candidate = info
                    .and_then(|info| info.last_token_usage)
                    .and_then(|usage| event(timestamp, &context, usage, None, None));
                fallback.push((candidate, duplicate));
            }
            _ => scan.stats.non_usage_lines += 1,
        }
    }
    // A partially read file contributes only line-level diagnostics; usage and
    // snapshots are committed together after the reader reaches EOF.
    scan.stats.rate_limit_snapshots += rate_limits.len() as u64;
    scan.rate_limits.extend(rate_limits);
    if has_record {
        scan.stats.files_with_records += 1;
        scan.stats.non_usage_lines += fallback.len() as u64;
        for event in records {
            if let Some(id) = &event.request_id {
                if !seen_responses.insert(id.clone()) {
                    scan.stats.deduplicated_lines += 1;
                    continue;
                }
            }
            scan.events.push(event);
        }
    } else {
        if has_token_count {
            scan.stats.files_token_count_only += 1;
        }
        for (candidate, duplicate) in fallback {
            match candidate {
                Some(_) if duplicate => {
                    scan.stats.token_count_usage_lines += 1;
                    scan.stats.deduplicated_lines += 1;
                }
                Some(event) => {
                    scan.stats.token_count_usage_lines += 1;
                    scan.events.push(event);
                }
                None => scan.stats.non_usage_lines += 1,
            }
        }
    }
    Ok(())
}

fn collect_paths(directory: &Path, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if entry.file_name().to_string_lossy().starts_with("rollout-")
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "jsonl")
        {
            paths.push(entry.path());
        } else if kind.is_dir() {
            collect_paths(&entry.path(), paths)?;
        }
    }
    Ok(())
}

/// Scans Codex rollouts recursively, using sorted paths and stable timestamp order.
/// A missing sessions directory is empty; individual file failures are counted.
pub fn scan_codex(home: &Path) -> Result<CodexScan, io::Error> {
    let mut paths = Vec::new();
    for directory in ["sessions", "archived_sessions"] {
        let path = home.join(".codex").join(directory);
        match fs::metadata(&path) {
            Ok(_) => collect_paths(&path, &mut paths)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    paths.sort();
    let mut scan = CodexScan::default();
    let mut seen_responses = HashSet::new();
    for path in paths {
        scan.stats.files += 1;
        if scan_file(&path, &mut scan, &mut seen_responses).is_err() {
            scan.stats.unreadable_files += 1;
        }
    }
    scan.events.sort_by_key(|event| event.timestamp);
    scan.rate_limits.sort_by_key(|snapshot| snapshot.timestamp);
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingRead<'a> {
        data: &'a [u8],
        offset: usize,
        fail_at: usize,
    }

    impl io::Read for FailingRead<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.offset == self.fail_at {
                return Err(io::Error::other("simulated read failure"));
            }
            let available = &self.data[self.offset..self.fail_at];
            let count = available.len().min(buf.len());
            buf[..count].copy_from_slice(&available[..count]);
            self.offset += count;
            Ok(count)
        }
    }

    #[test]
    fn partial_read_does_not_commit_usage_snapshots_or_response_ids() {
        let data = include_bytes!("../tests/fixtures/codex/new.jsonl");
        let fail_at = data
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b'\n')
            .nth(3)
            .map(|(index, _)| index + 1)
            .expect("fixture has a rate snapshot after a usage record");
        let path = Path::new("rollout-simulated.jsonl");
        let mut scan = CodexScan::default();
        let mut seen_responses = HashSet::new();

        let reader = BufReader::new(FailingRead {
            data,
            offset: 0,
            fail_at,
        });
        assert!(scan_reader(path, reader, &mut scan, &mut seen_responses).is_err());
        assert_eq!(scan.stats.total_lines, 4);
        assert_eq!(scan.stats.usage_records, 1);
        assert_eq!(scan.stats.rate_limit_snapshots, 0);
        assert!(scan.events.is_empty());
        assert!(scan.rate_limits.is_empty());
        assert!(seen_responses.is_empty());

        scan_reader(path, io::Cursor::new(data), &mut scan, &mut seen_responses)
            .expect("scan complete fixture after failed read");
        assert_eq!(scan.events.len(), 2);
        assert_eq!(scan.rate_limits.len(), 2);
        assert_eq!(scan.stats.rate_limit_snapshots, 2);
        assert_eq!(scan.stats.deduplicated_lines, 0);
        assert!(seen_responses.contains("resp-first"));
    }
}
