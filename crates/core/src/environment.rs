//! Read-only inventory of startup configuration. Values and instruction text are discarded.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use serde::de::IgnoredAny;
use serde::Deserialize;

#[derive(Clone, Debug, Default)]
pub struct EnvironmentSnapshot {
    pub claude_global_mcp_names: Vec<String>,
    pub claude_project_mcp_names: BTreeMap<PathBuf, Vec<String>>,
    pub codex_global_mcp_names: Vec<String>,
    pub claude_plugins: usize,
    pub claude_instruction_bytes: u64,
    pub codex_instruction_bytes: u64,
    pub claude_project_instruction_bytes: BTreeMap<PathBuf, u64>,
    pub codex_project_instruction_bytes: BTreeMap<PathBuf, u64>,
}

#[derive(Deserialize)]
struct ClaudeConfig {
    #[serde(default, rename = "mcpServers")]
    mcp_servers: BTreeMap<String, IgnoredAny>,
    #[serde(default)]
    projects: BTreeMap<String, ProjectConfig>,
}

#[derive(Deserialize)]
struct ProjectConfig {
    #[serde(default, rename = "mcpServers")]
    mcp_servers: BTreeMap<String, IgnoredAny>,
    #[serde(default, rename = "disabledMcpServers")]
    disabled_mcp_servers: Vec<String>,
}

#[derive(Deserialize)]
struct ClaudeSettings {
    #[serde(default, rename = "enabledPlugins")]
    enabled_plugins: BTreeMap<String, bool>,
}

#[derive(Deserialize)]
struct McpFile {
    #[serde(default, rename = "mcpServers")]
    mcp_servers: BTreeMap<String, IgnoredAny>,
}

#[derive(Deserialize)]
struct CodexConfig {
    #[serde(default)]
    mcp_servers: BTreeMap<String, CodexMcp>,
}

#[derive(Deserialize)]
struct CodexMcp {
    enabled: Option<bool>,
}

fn bytes(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |metadata| metadata.len())
}

/// `home` and project roots are injected by the caller, including in tests.
pub fn scan_environment(home: &Path, projects: &[PathBuf]) -> EnvironmentSnapshot {
    let mut snapshot = EnvironmentSnapshot::default();
    let mut claude_names = BTreeSet::new();
    let mut disabled_by_project = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    if let Ok(file) = File::open(home.join(".claude.json")) {
        if let Ok(mut config) = serde_json::from_reader::<_, ClaudeConfig>(file) {
            claude_names.extend(config.mcp_servers.into_keys());
            for project in projects {
                if let Some(config) = config
                    .projects
                    .remove(&project.to_string_lossy().into_owned())
                {
                    let disabled: BTreeSet<_> = config.disabled_mcp_servers.into_iter().collect();
                    snapshot.claude_project_mcp_names.insert(
                        project.clone(),
                        config
                            .mcp_servers
                            .into_keys()
                            .filter(|name| !disabled.contains(name))
                            .collect(),
                    );
                    disabled_by_project.insert(project.clone(), disabled);
                }
            }
        }
    }
    if let Ok(file) = File::open(home.join(".claude/settings.json")) {
        if let Ok(settings) = serde_json::from_reader::<_, ClaudeSettings>(file) {
            snapshot.claude_plugins = settings
                .enabled_plugins
                .values()
                .filter(|enabled| **enabled)
                .count();
        }
    }
    if let Ok(contents) = fs::read_to_string(home.join(".codex/config.toml")) {
        if let Ok(config) = toml::from_str::<CodexConfig>(&contents) {
            snapshot.codex_global_mcp_names = config
                .mcp_servers
                .into_iter()
                .filter(|(_, value)| value.enabled != Some(false))
                .map(|(name, _)| name)
                .collect();
        }
    }
    snapshot.claude_instruction_bytes = bytes(&home.join(".claude/CLAUDE.md"));
    snapshot.codex_instruction_bytes = bytes(&home.join(".codex/AGENTS.md"));
    for project in projects {
        if let Ok(file) = File::open(project.join(".mcp.json")) {
            if let Ok(config) = serde_json::from_reader::<_, McpFile>(file) {
                snapshot
                    .claude_project_mcp_names
                    .entry(project.clone())
                    .or_default()
                    .extend(config.mcp_servers.into_keys());
            }
        }
        snapshot
            .claude_project_instruction_bytes
            .insert(project.clone(), bytes(&project.join("CLAUDE.md")));
        snapshot
            .codex_project_instruction_bytes
            .insert(project.clone(), bytes(&project.join("AGENTS.md")));
    }
    for names in snapshot.claude_project_mcp_names.values_mut() {
        names.sort();
        names.dedup();
    }
    for (project, disabled) in disabled_by_project {
        if let Some(names) = snapshot.claude_project_mcp_names.get_mut(&project) {
            names.retain(|name| !disabled.contains(name));
        }
    }
    snapshot.claude_global_mcp_names = claude_names.into_iter().collect();
    snapshot
}
