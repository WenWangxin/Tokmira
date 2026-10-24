use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::claude_quota::{ClaudeQuota, WeeklyBreakdown};
use crate::codex_app_server::CodexQuota;
use crate::recorder::ReadStats;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredWindow {
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub window_minutes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClaudeSnapshot {
    pub at: DateTime<Utc>,
    pub subscription_type: Option<String>,
    pub five_hour: Option<StoredWindow>,
    pub seven_day: Option<StoredWindow>,
    pub weekly_breakdown: Option<WeeklyBreakdown>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CodexSnapshot {
    pub at: DateTime<Utc>,
    #[serde(default)]
    pub limit_id: Option<String>,
    pub plan_type: Option<String>,
    pub primary: Option<StoredWindow>,
    pub secondary: Option<StoredWindow>,
    pub reset_credits_available: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexDaily {
    pub date: NaiveDate,
    pub tokens: u64,
}

fn append<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_vec(value).map_err(io::Error::other)?;
    line.push(b'\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(&line)
}

fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> (Vec<T>, ReadStats) {
    let mut stats = ReadStats::default();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return (Vec::new(), stats),
        Err(_) => {
            stats.unreadable_files = 1;
            return (Vec::new(), stats);
        }
    };
    let mut records = Vec::new();
    for line in BufReader::new(file).lines() {
        stats.total_lines += 1;
        match line {
            Ok(line) => match serde_json::from_str::<T>(&line) {
                Ok(record) => records.push(record),
                Err(_) => stats.malformed_lines += 1,
            },
            Err(_) => {
                stats.malformed_lines += 1;
                break;
            }
        }
    }
    (records, stats)
}

fn quota_path(data_dir: &Path, name: &str) -> std::path::PathBuf {
    data_dir.join("quota").join(name)
}

pub fn append_claude(data_dir: &Path, snapshot: &ClaudeSnapshot) -> io::Result<()> {
    append(&quota_path(data_dir, "claude.jsonl"), snapshot)
}
pub fn append_codex(data_dir: &Path, snapshot: &CodexSnapshot) -> io::Result<()> {
    append(&quota_path(data_dir, "codex.jsonl"), snapshot)
}
pub fn read_claude(data_dir: &Path) -> (Vec<ClaudeSnapshot>, ReadStats) {
    read(&quota_path(data_dir, "claude.jsonl"))
}
pub fn read_codex(data_dir: &Path) -> (Vec<CodexSnapshot>, ReadStats) {
    read(&quota_path(data_dir, "codex.jsonl"))
}

pub fn write_codex_daily(data_dir: &Path, days: &[CodexDaily]) -> io::Result<()> {
    let path = quota_path(data_dir, "codex-daily.json");
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("invalid quota path"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".codex-daily-{}.tmp", std::process::id()));
    fs::write(&temp, serde_json::to_vec(days).map_err(io::Error::other)?)?;
    fs::rename(temp, path)
}

pub fn read_codex_daily(data_dir: &Path) -> io::Result<Vec<CodexDaily>> {
    let path = quota_path(data_dir, "codex-daily.json");
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

impl From<&ClaudeQuota> for ClaudeSnapshot {
    fn from(quota: &ClaudeQuota) -> Self {
        let window = |name: &str, minutes| {
            quota
                .windows
                .iter()
                .find(|w| w.name == name)
                .map(|w| StoredWindow {
                    used_percent: w.utilization,
                    resets_at: w.resets_at,
                    window_minutes: Some(minutes),
                })
        };
        Self {
            at: quota.fetched_at,
            subscription_type: quota.subscription_type.clone(),
            five_hour: window("five_hour", 300),
            seven_day: window("seven_day", 10080),
            weekly_breakdown: quota.weekly_breakdown.clone(),
        }
    }
}
impl From<&CodexQuota> for CodexSnapshot {
    fn from(quota: &CodexQuota) -> Self {
        let window = |w: &crate::codex::RateLimitWindow| StoredWindow {
            used_percent: w.used_percent,
            resets_at: w.resets_at,
            window_minutes: w.window_minutes,
        };
        Self {
            at: quota.fetched_at,
            limit_id: quota.limit_id.clone(),
            plan_type: quota.plan_type.clone(),
            primary: quota.primary.as_ref().map(window),
            secondary: quota.secondary.as_ref().map(window),
            reset_credits_available: quota.reset_credits.as_ref().and_then(|r| r.available_count),
        }
    }
}
