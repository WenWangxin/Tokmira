use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Error};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::model::{Source, TokenCounts, Tool, UsageEvent};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ClaudeScanStats {
    pub files: u64,
    pub total_lines: u64,
    pub valid_usage_lines: u64,
    pub deduplicated_lines: u64,
    pub malformed_lines: u64,
    pub non_usage_lines: u64,
    pub unreadable_files: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ClaudeScan {
    pub events: Vec<UsageEvent>,
    pub stats: ClaudeScanStats,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ClaudeDebugSummary {
    pub stats: ClaudeScanStats,
    pub events_total: usize,
    pub unique_sessions: usize,
    pub tokens: TokenCounts,
    pub sources: ClaudeSourceCounts,
    pub models: BTreeMap<String, ClaudeModelCounts>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ClaudeSourceCounts {
    pub claude_cli: u64,
    pub claude_desktop: u64,
    pub unknown: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ClaudeModelCounts {
    pub events: u64,
    pub output: u64,
}

impl ClaudeScan {
    #[must_use]
    pub fn debug_summary(&self) -> ClaudeDebugSummary {
        let mut summary = ClaudeDebugSummary {
            stats: self.stats.clone(),
            events_total: self.events.len(),
            ..ClaudeDebugSummary::default()
        };
        let mut sessions = HashSet::new();
        for event in &self.events {
            sessions.insert(&event.session_id);
            summary.tokens.add_saturating(&event.tokens);
            match event.source {
                Source::ClaudeCli => summary.sources.claude_cli += 1,
                Source::ClaudeDesktop => summary.sources.claude_desktop += 1,
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
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    timestamp: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    entrypoint: Option<String>,
    message: Option<RawMessage>,
}

struct RawMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<RawUsage>,
    invalid: bool,
}

impl RawMessage {
    fn invalid() -> Self {
        Self {
            id: None,
            model: None,
            usage: None,
            invalid: true,
        }
    }
}

impl<'de> Deserialize<'de> for RawMessage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RawMessageVisitor;

        impl<'de> Visitor<'de> for RawMessageVisitor {
            type Value = RawMessage;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a message object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut message = RawMessage {
                    id: None,
                    model: None,
                    usage: None,
                    invalid: false,
                };
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "id" => message.id = map.next_value()?,
                        "model" => message.model = map.next_value()?,
                        "usage" => message.usage = map.next_value()?,
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(message)
            }

            fn visit_str<E: serde::de::Error>(self, _value: &str) -> Result<Self::Value, E> {
                Ok(RawMessage::invalid())
            }

            fn visit_bool<E: serde::de::Error>(self, _value: bool) -> Result<Self::Value, E> {
                Ok(RawMessage::invalid())
            }

            fn visit_i64<E: serde::de::Error>(self, _value: i64) -> Result<Self::Value, E> {
                Ok(RawMessage::invalid())
            }

            fn visit_u64<E: serde::de::Error>(self, _value: u64) -> Result<Self::Value, E> {
                Ok(RawMessage::invalid())
            }

            fn visit_f64<E: serde::de::Error>(self, _value: f64) -> Result<Self::Value, E> {
                Ok(RawMessage::invalid())
            }

            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(RawMessage::invalid())
            }
        }

        deserializer.deserialize_any(RawMessageVisitor)
    }
}

#[derive(Deserialize)]
struct RawUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    cache_creation: Option<RawCacheCreation>,
}

#[derive(Deserialize)]
struct RawCacheCreation {
    #[serde(default)]
    ephemeral_5m_input_tokens: u64,
    #[serde(default)]
    ephemeral_1h_input_tokens: u64,
}

enum ParsedLine {
    Event(Box<UsageEvent>, Option<DedupKey>),
    NonUsage,
    Malformed,
}

#[derive(Hash, PartialEq, Eq)]
enum DedupKey {
    Both(String, String),
    Message(String),
    Request(String),
}

fn parse_line(line: &[u8]) -> ParsedLine {
    let raw: RawLine = match serde_json::from_slice(line) {
        Ok(raw) => raw,
        Err(_) => return ParsedLine::Malformed,
    };
    if raw.kind.as_deref() != Some("assistant") {
        return ParsedLine::NonUsage;
    }
    let Some(message) = raw.message else {
        return ParsedLine::NonUsage;
    };
    if message.invalid {
        return ParsedLine::Malformed;
    }
    let Some(usage) = message.usage else {
        return ParsedLine::NonUsage;
    };
    if message.model.as_deref() == Some("<synthetic>") {
        return ParsedLine::NonUsage;
    }
    let (cache_write_5m, cache_write_1h, cache_write_unspecified) = match usage.cache_creation {
        Some(cache) => (
            cache.ephemeral_5m_input_tokens,
            cache.ephemeral_1h_input_tokens,
            0,
        ),
        None => (0, 0, usage.cache_creation_input_tokens),
    };
    let tokens = TokenCounts {
        input: usage.input_tokens,
        output: usage.output_tokens,
        reasoning: 0,
        cache_read: usage.cache_read_input_tokens,
        cache_write_5m,
        cache_write_1h,
        cache_write_unspecified,
    };
    if tokens.is_zero() && usage.cache_creation_input_tokens == 0 {
        return ParsedLine::NonUsage;
    }
    let (Some(session_id), Some(timestamp), Some(model)) =
        (raw.session_id, raw.timestamp, message.model)
    else {
        return ParsedLine::Malformed;
    };
    let timestamp = match DateTime::parse_from_rfc3339(&timestamp) {
        Ok(timestamp) => timestamp.with_timezone(&Utc),
        Err(_) => return ParsedLine::Malformed,
    };
    if session_id.is_empty() || model.is_empty() {
        return ParsedLine::Malformed;
    }

    let message_id = message.id.filter(|id| !id.is_empty());
    let request_id = raw.request_id.filter(|id| !id.is_empty());
    let dedup_key = match (&message_id, &request_id) {
        (Some(message), Some(request)) => Some(DedupKey::Both(message.clone(), request.clone())),
        (Some(message), None) => Some(DedupKey::Message(message.clone())),
        (None, Some(request)) => Some(DedupKey::Request(request.clone())),
        (None, None) => None,
    };
    let source = match raw.entrypoint.as_deref() {
        Some("cli") => Source::ClaudeCli,
        Some("claude-desktop") => Source::ClaudeDesktop,
        _ => Source::Unknown,
    };
    ParsedLine::Event(
        Box::new(UsageEvent {
            tool: Tool::Claude,
            source,
            session_id,
            request_id,
            timestamp,
            model,
            project: raw.cwd,
            is_subagent: raw.is_sidechain == Some(true),
            tokens,
        }),
        dedup_key,
    )
}

fn scan_file(
    path: &Path,
    scan: &mut ClaudeScan,
    dedup: &mut HashMap<DedupKey, usize>,
) -> Result<(), Error> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        scan.stats.total_lines += 1;
        match parse_line(&line) {
            ParsedLine::NonUsage => scan.stats.non_usage_lines += 1,
            ParsedLine::Malformed => scan.stats.malformed_lines += 1,
            ParsedLine::Event(event, key) => {
                scan.stats.valid_usage_lines += 1;
                if let Some(key) = key {
                    if let Some(&index) = dedup.get(&key) {
                        scan.stats.deduplicated_lines += 1;
                        if event.tokens.output > scan.events[index].tokens.output {
                            scan.events[index] = *event;
                        }
                    } else {
                        dedup.insert(key, scan.events.len());
                        scan.events.push(*event);
                    }
                } else {
                    scan.events.push(*event);
                }
            }
        }
    }
    Ok(())
}

/// Scans only direct JSONL children of each Claude project directory.
///
/// A missing projects directory is an empty scan. Failure to enumerate an existing
/// projects directory is returned to the caller; individual file failures are counted.
pub fn scan_claude(home: &Path) -> Result<ClaudeScan, Error> {
    let projects = home.join(".claude").join("projects");
    let project_entries = match fs::read_dir(projects) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ClaudeScan::default());
        }
        Err(error) => return Err(error),
    };
    let mut scan = ClaudeScan::default();
    let mut dedup = HashMap::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    for project in project_entries {
        let project = project?;
        if !project.file_type()?.is_dir() {
            continue;
        }
        let files = match fs::read_dir(project.path()) {
            Ok(files) => files,
            Err(_) => continue,
        };
        for file in files {
            let file = match file {
                Ok(file) => file,
                Err(_) => continue,
            };
            let path = file.path();
            if path
                .extension()
                .is_none_or(|extension| extension != "jsonl")
            {
                continue;
            }
            paths.push(path);
        }
    }
    paths.sort();
    for path in paths {
        scan.stats.files += 1;
        if scan_file(&path, &mut scan, &mut dedup).is_err() {
            scan.stats.unreadable_files += 1;
        }
    }
    scan.events.sort_by_key(|event| event.timestamp);
    Ok(scan)
}
