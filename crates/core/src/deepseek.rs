//! Local, read-only DeepSeek Harness v4 session usage.
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::model::{Source, TokenCounts, Tool, UsageEvent};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DeepSeekScanStats {
    pub files: u64,
    pub unsupported_files: u64,
    pub truncated_files: u64,
    pub unreadable_files: u64,
    pub total_lines: u64,
    pub malformed_lines: u64,
    pub non_usage_lines: u64,
    pub usage_samples: u64,
    pub replaced_samples: u64,
    pub retries: u64,
    pub seeded_events_skipped: u64,
    pub title_requests: u64,
    pub custom_route_events: u64,
}

/// A billed request Harness makes to name a session. The log records no usage
/// for it, so only its size is kept, never its content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DeepSeekTitleCall {
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub model: String,
    pub provider: Option<String>,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DeepSeekScan {
    pub events: Vec<UsageEvent>,
    pub titles: BTreeMap<String, String>,
    pub session_providers: BTreeMap<String, BTreeSet<String>>,
    pub title_calls: Vec<DeepSeekTitleCall>,
    /// Sessions whose log Harness uploaded to DeepSeek, with the accepted uploads.
    pub uploads: BTreeMap<String, u64>,
    /// Requests sent through a route other than DeepSeek's own, by session and time;
    /// their tokens are exact but official prices do not apply.
    pub custom_route: BTreeSet<(String, DateTime<Utc>)>,
    pub stats: DeepSeekScanStats,
}

/// Whether a provider id is one of DeepSeek's own billed routes.
#[must_use]
pub fn is_official_route(provider: Option<&str>) -> bool {
    matches!(provider, Some("deepseek-account" | "deepseek-official"))
}

impl DeepSeekScan {
    #[must_use]
    pub fn is_custom_route(&self, event: &UsageEvent) -> bool {
        self.custom_route
            .contains(&(event.session_id.clone(), event.timestamp))
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct DeepSeekDebugSummary {
    pub stats: DeepSeekScanStats,
    pub events_total: usize,
    pub unique_sessions: usize,
    pub tokens: TokenCounts,
    pub models: BTreeMap<String, u64>,
    pub session_providers: BTreeMap<String, BTreeSet<String>>,
}

impl DeepSeekScan {
    #[must_use]
    pub fn debug_summary(&self) -> DeepSeekDebugSummary {
        let mut summary = DeepSeekDebugSummary {
            stats: self.stats.clone(),
            events_total: self.events.len(),
            session_providers: self.session_providers.clone(),
            ..DeepSeekDebugSummary::default()
        };
        let mut sessions = BTreeSet::new();
        for event in &self.events {
            sessions.insert(&event.session_id);
            summary.tokens.add_saturating(&event.tokens);
            *summary.models.entry(event.model.clone()).or_default() += 1;
        }
        summary.unique_sessions = sessions.len();
        summary
    }
}

/// Resolve DSH_HOME without reading the process environment.
#[must_use]
pub fn deepseek_root(home: &Path, dsh_home: Option<&OsStr>) -> PathBuf {
    dsh_home
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".dsh"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    #[serde(rename = "type")]
    kind: String,
    version: u32,
    id: String,
    cwd: Option<String>,
    #[serde(default)]
    is_seeded: bool,
    #[serde(default)]
    delegation_depth: u32,
}

#[derive(Deserialize)]
struct KindLine {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: String,
    time: Option<i64>,
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    turn: Option<i64>,
    step: Option<i64>,
    usage: Option<RawUsage>,
    #[serde(default, deserialize_with = "last_stream_usage")]
    stream: Option<RawUsage>,
    message: Option<Message>,
    header: Option<RequestHeader>,
    title: Option<String>,
    route: Option<Route>,
}

/// Only the route of a title request; its system prompt and messages are skipped.
#[derive(Clone, Deserialize)]
struct Route {
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawUsage {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    cache_read_tokens: u64,
    #[serde(default)]
    cache_write_tokens: u64,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    source: Option<MessageSource>,
}
#[derive(Deserialize)]
struct MessageSource {
    model: Option<String>,
}
#[derive(Deserialize)]
struct RequestHeader {
    config: Option<RequestConfig>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestConfig {
    model: Option<String>,
    provider: Option<String>,
    reasoning_effort: Option<String>,
}
#[derive(Deserialize)]
struct StreamChunk {
    #[serde(rename = "type")]
    kind: Option<String>,
    chunk: Option<UsageChunk>,
}
#[derive(Deserialize)]
struct UsageChunk {
    #[serde(rename = "type")]
    kind: Option<String>,
    usage: Option<RawUsage>,
}

fn last_stream_usage<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<RawUsage>, D::Error> {
    struct LastUsage;
    struct OptionalLastUsage;
    impl<'de> Visitor<'de> for LastUsage {
        type Value = Option<RawUsage>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an array of stream chunks")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut last = None;
            while let Some(chunk) = seq.next_element::<StreamChunk>()? {
                if chunk.kind.as_deref() == Some("chunk") {
                    if let Some(body) = chunk.chunk {
                        if body.kind.as_deref() == Some("usage") {
                            if let Some(usage) = body.usage {
                                last = Some(usage);
                            }
                        }
                    }
                }
            }
            Ok(last)
        }
    }
    impl<'de> Visitor<'de> for OptionalLastUsage {
        type Value = Option<RawUsage>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an array of stream chunks or null")
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_seq(LastUsage)
        }
    }
    deserializer.deserialize_option(OptionalLastUsage)
}

#[derive(Clone)]
struct Slot {
    event: UsageEvent,
    provider: Option<String>,
    usage: RawUsage,
}
#[derive(Clone, Default)]
struct FileScan {
    header: Option<HeaderState>,
    slots: Vec<Slot>,
    active: BTreeMap<(i64, i64), usize>,
    providers: BTreeSet<String>,
    title: Option<String>,
    title_calls: Vec<(DateTime<Utc>, Route, u64)>,
    uploads: u64,
    current_model: Option<String>,
    current_effort: Option<String>,
    current_provider: Option<String>,
    seeded: bool,
    unsupported: bool,
    stats: DeepSeekScanStats,
}
#[derive(Clone)]
struct HeaderState {
    id: String,
    cwd: Option<String>,
    subagent: bool,
}

impl FileScan {
    fn line(&mut self, line: &[u8]) {
        self.stats.total_lines += 1;
        if self.unsupported {
            return;
        }
        if self.header.is_none() {
            match serde_json::from_slice::<Header>(line) {
                Ok(header)
                    if header.kind == "session" && header.version == 4 && !header.id.is_empty() =>
                {
                    self.seeded = header.is_seeded;
                    self.header = Some(HeaderState {
                        id: header.id,
                        cwd: header.cwd,
                        subagent: header.delegation_depth > 0,
                    });
                }
                Ok(header) if header.kind == "session" && header.version != 4 => {
                    self.unsupported = true;
                    self.stats.unsupported_files = 1;
                }
                Ok(_) => self.stats.malformed_lines += 1,
                Err(_) => self.stats.malformed_lines += 1,
            }
            return;
        }
        let kind = match serde_json::from_slice::<KindLine>(line) {
            Ok(raw) => raw.kind,
            Err(_) => {
                self.stats.malformed_lines += 1;
                return;
            }
        };
        if self.seeded {
            self.stats.seeded_events_skipped += 1;
            if kind == "session/end-seed" {
                self.seeded = false;
            }
            return;
        }
        if kind == "session/title-llm-request" {
            // Every title request is billed, including re-titles later in the session.
            self.stats.title_requests += 1;
            self.stats.non_usage_lines += 1;
            if let Ok(raw) = serde_json::from_slice::<Line>(line) {
                let route = raw.data.and_then(|data| data.route);
                if let Some(at) = raw.time.and_then(DateTime::<Utc>::from_timestamp_millis) {
                    self.title_calls.push((
                        at,
                        route.unwrap_or(Route {
                            provider: None,
                            model: None,
                        }),
                        line.len() as u64,
                    ));
                }
            }
            return;
        }
        if kind == "session-log-deepseek/delivery-accepted" {
            self.uploads += 1;
            self.stats.non_usage_lines += 1;
            return;
        }
        if !matches!(
            kind.as_str(),
            "request/header"
                | "assistant/attempt"
                | "assistant/message"
                | "llm/retry-started"
                | "session/title"
        ) {
            self.stats.non_usage_lines += 1;
            return;
        }
        let raw = match serde_json::from_slice::<Line>(line) {
            Ok(raw) => raw,
            Err(_) => {
                self.stats.malformed_lines += 1;
                return;
            }
        };
        let data = raw.data;
        match raw.kind.as_str() {
            "request/header" => {
                if let Some(config) = data
                    .and_then(|data| data.header)
                    .and_then(|header| header.config)
                {
                    self.current_model = config.model;
                    self.current_effort = config.reasoning_effort;
                    self.current_provider = config.provider;
                    if let Some(provider) = &self.current_provider {
                        self.providers.insert(provider.clone());
                    }
                }
                self.stats.non_usage_lines += 1;
            }
            "assistant/attempt" | "assistant/message" => {
                let Some(data) = data else {
                    self.stats.non_usage_lines += 1;
                    return;
                };
                let Some(usage) = data.usage.or(data.stream) else {
                    self.stats.non_usage_lines += 1;
                    return;
                };
                self.stats.usage_samples += 1;
                let Some((turn, step, at, header)) = data
                    .turn
                    .zip(data.step)
                    .zip(raw.time.and_then(DateTime::<Utc>::from_timestamp_millis))
                    .and_then(|((turn, step), at)| {
                        self.header.as_ref().map(|header| (turn, step, at, header))
                    })
                else {
                    self.stats.malformed_lines += 1;
                    return;
                };
                let tokens = TokenCounts {
                    input: usage.input_tokens,
                    output: usage.output_tokens,
                    cache_read: usage.cache_read_tokens,
                    cache_write_unspecified: usage.cache_write_tokens,
                    ..TokenCounts::default()
                };
                let message = data.message;
                let event = UsageEvent {
                    tool: Tool::DeepSeek,
                    source: Source::DeepSeek,
                    session_id: header.id.clone(),
                    request_id: message.as_ref().and_then(|value| value.id.clone()),
                    timestamp: at,
                    model: message
                        .and_then(|value| value.source)
                        .and_then(|source| source.model)
                        .or_else(|| self.current_model.clone())
                        .unwrap_or_default(),
                    project: header.cwd.clone(),
                    is_subagent: header.subagent,
                    fast: false,
                    effort: self.current_effort.clone(),
                    web_search_requests: 0,
                    tokens,
                };
                let key = (turn, step);
                if let Some(index) = self.active.get(&key).copied() {
                    if self.slots[index].usage == usage {
                        return;
                    }
                    self.stats.replaced_samples += 1;
                    self.slots[index] = Slot {
                        event,
                        provider: self.current_provider.clone(),
                        usage,
                    };
                } else {
                    self.active.insert(key, self.slots.len());
                    self.slots.push(Slot {
                        event,
                        provider: self.current_provider.clone(),
                        usage,
                    });
                }
            }
            "llm/retry-started" => {
                if let Some(key) = data.and_then(|data| data.turn.zip(data.step)) {
                    if self.active.remove(&key).is_some() {
                        self.stats.retries += 1;
                    }
                }
                self.stats.non_usage_lines += 1;
            }
            "session/title" => {
                if let Some(title) = data.and_then(|data| data.title) {
                    self.title = Some(title);
                }
                self.stats.non_usage_lines += 1;
            }
            _ => self.stats.non_usage_lines += 1,
        }
    }
}

fn parse_file(path: &Path) -> FileScan {
    let mut result = FileScan::default();
    let Ok(file) = File::open(path) else {
        result.stats.unreadable_files = 1;
        return result;
    };
    let mut input = BufReader::new(file);
    let mut pending = Vec::new();
    loop {
        match input.fill_buf() {
            Ok([]) => break,
            Ok(_) => {}
            Err(_) => {
                result.stats.unreadable_files = 1;
                break;
            }
        }
        // Decode the whole frame before using any of it, so a frame Harness is
        // still writing is dropped as a unit and earlier frames stay counted.
        let mut frame = Vec::new();
        let decoded = ruzstd::decoding::StreamingDecoder::new(&mut input)
            .map_err(|_| ())
            .and_then(|mut decoder| decoder.read_to_end(&mut frame).map_err(|_| ()));
        if decoded.is_err() {
            result.stats.truncated_files = 1;
            break;
        }
        pending.extend_from_slice(&frame);
        let mut start = 0;
        while let Some(offset) = pending[start..].iter().position(|byte| *byte == b'\n') {
            let end = start + offset + 1;
            result.line(&pending[start..end]);
            start = end;
        }
        pending.drain(..start);
    }
    if result.stats.truncated_files == 0
        && result.stats.unreadable_files == 0
        && !pending.is_empty()
    {
        result.line(&pending);
    }
    result
}

fn paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let sessions = root.join("sessions");
    let workspaces = match fs::read_dir(sessions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(paths),
        Err(error) => return Err(error),
    };
    for workspace in workspaces {
        let workspace = workspace?;
        if !workspace.file_type()?.is_dir() {
            continue;
        }
        for session in fs::read_dir(workspace.path())? {
            let session = session?;
            if !session.file_type()?.is_dir()
                || !session
                    .file_name()
                    .to_string_lossy()
                    .starts_with("session-")
            {
                continue;
            }
            for entry in fs::read_dir(session.path())? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with("session.v") && name.ends_with(".jsonl.zstd") {
                        paths.push(entry.path());
                    }
                }
            }
        }
    }
    paths.sort();
    Ok(paths)
}

#[derive(Default)]
pub struct DeepSeekScanner {
    files: BTreeMap<PathBuf, (crate::claude::Fingerprint, FileScan)>,
    changed: bool,
}
impl DeepSeekScanner {
    #[must_use]
    pub fn changed(&self) -> bool {
        self.changed
    }

    pub fn scan(&mut self, root: &Path) -> io::Result<DeepSeekScan> {
        self.changed = false;
        let mut previous = std::mem::take(&mut self.files);
        for path in paths(root)? {
            let fingerprint = crate::claude::fingerprint(&path);
            let parsed = match previous.remove(&path) {
                Some((old, parsed)) if old.is_some() && old == fingerprint => parsed,
                _ => {
                    self.changed = true;
                    if path
                        .file_name()
                        .is_some_and(|name| name == "session.v4.jsonl.zstd")
                    {
                        parse_file(&path)
                    } else {
                        let mut file = FileScan::default();
                        file.stats.unsupported_files = 1;
                        file
                    }
                }
            };
            self.files.insert(path, (fingerprint, parsed));
        }
        self.changed |= !previous.is_empty();
        let mut scan = DeepSeekScan::default();
        for (_, file) in self.files.values() {
            scan.stats.files += 1;
            let s = &file.stats;
            scan.stats.unsupported_files += s.unsupported_files;
            scan.stats.truncated_files += s.truncated_files;
            scan.stats.unreadable_files += s.unreadable_files;
            scan.stats.total_lines += s.total_lines;
            scan.stats.malformed_lines += s.malformed_lines;
            scan.stats.non_usage_lines += s.non_usage_lines;
            scan.stats.usage_samples += s.usage_samples;
            scan.stats.replaced_samples += s.replaced_samples;
            scan.stats.retries += s.retries;
            scan.stats.seeded_events_skipped += s.seeded_events_skipped;
            scan.stats.title_requests += s.title_requests;
            if let Some(header) = &file.header {
                if let Some(title) = &file.title {
                    scan.titles.insert(header.id.clone(), title.clone());
                }
                scan.session_providers
                    .entry(header.id.clone())
                    .or_default()
                    .extend(file.providers.iter().cloned());
                for slot in &file.slots {
                    if !is_official_route(slot.provider.as_deref()) {
                        scan.stats.custom_route_events += 1;
                        scan.custom_route
                            .insert((slot.event.session_id.clone(), slot.event.timestamp));
                    }
                    scan.events.push(slot.event.clone());
                }
                for (at, route, bytes) in &file.title_calls {
                    scan.title_calls.push(DeepSeekTitleCall {
                        session_id: header.id.clone(),
                        timestamp: *at,
                        model: route.model.clone().unwrap_or_default(),
                        provider: route.provider.clone(),
                        bytes: *bytes,
                    });
                }
                if file.uploads > 0 {
                    *scan.uploads.entry(header.id.clone()).or_default() += file.uploads;
                }
            }
        }
        scan.events.sort_by_key(|event| event.timestamp);
        scan.title_calls.sort_by_key(|call| call.timestamp);
        Ok(scan)
    }
}

pub fn scan_deepseek(root: &Path) -> io::Result<DeepSeekScan> {
    DeepSeekScanner::default().scan(root)
}
