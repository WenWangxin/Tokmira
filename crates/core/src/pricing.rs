use std::collections::BTreeMap;

use serde::Deserialize;

use crate::model::TokenCounts;

#[derive(Clone, Debug, Deserialize)]
pub struct ModelPrices {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_write_unspecified: f64,
}

impl ModelPrices {
    fn valid(&self) -> bool {
        [
            self.input,
            self.output,
            self.cache_read,
            self.cache_write_5m,
            self.cache_write_1h,
            self.cache_write_unspecified,
        ]
        .iter()
        .all(|price| price.is_finite() && *price >= 0.0)
    }

    #[must_use]
    pub fn cost_usd(&self, tokens: &TokenCounts) -> f64 {
        (tokens.input as f64 * self.input
            + tokens.output as f64 * self.output
            + tokens.cache_read as f64 * self.cache_read
            + tokens.cache_write_5m as f64 * self.cache_write_5m
            + tokens.cache_write_1h as f64 * self.cache_write_1h
            + tokens.cache_write_unspecified as f64 * self.cache_write_unspecified)
            / 1_000_000.0
    }
}

#[derive(Clone, Debug, Deserialize)]
struct DefaultTable {
    version: String,
    currency: String,
    unit: String,
    note: String,
    sources: Vec<PricingSource>,
    models: BTreeMap<String, ModelPrices>,
    subscriptions: Subscriptions,
    server_tools: ServerTools,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ServerTools {
    pub web_search_per_1k: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Subscriptions {
    #[serde(default)]
    claude: BTreeMap<String, f64>,
    #[serde(default)]
    chatgpt: BTreeMap<String, f64>,
}

impl Subscriptions {
    fn valid(&self) -> bool {
        self.claude
            .values()
            .chain(self.chatgpt.values())
            .all(|price| price.is_finite() && *price >= 0.0)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct PricingSource {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct OverrideTable {
    version: Option<String>,
    currency: Option<String>,
    unit: Option<String>,
    note: Option<String>,
    sources: Option<Vec<PricingSource>>,
    #[serde(default)]
    models: BTreeMap<String, ModelPrices>,
    subscriptions: Option<Subscriptions>,
    server_tools: Option<ServerTools>,
}

#[derive(Clone, Debug)]
pub struct Pricing {
    pub version: String,
    pub currency: String,
    pub unit: String,
    pub note: String,
    pub sources: Vec<PricingSource>,
    models: BTreeMap<String, ModelPrices>,
    subscriptions: Subscriptions,
    pub server_tools: ServerTools,
}

impl Pricing {
    /// Parses the bundled, offline price table.
    pub fn bundled() -> Result<Self, &'static str> {
        let table: DefaultTable = toml::from_str(include_str!("../../../config/pricing.toml"))
            .map_err(|_| "内置价格表格式错误")?;
        if table.models.is_empty()
            || table.models.values().any(|prices| !prices.valid())
            || !table.subscriptions.valid()
            || !table.server_tools.web_search_per_1k.is_finite()
            || table.server_tools.web_search_per_1k < 0.0
        {
            return Err("内置价格表价格无效");
        }
        Ok(Self {
            version: table.version,
            currency: table.currency,
            unit: table.unit,
            note: table.note,
            sources: table.sources,
            models: table.models,
            subscriptions: table.subscriptions,
            server_tools: table.server_tools,
        })
    }

    /// Applies complete model records from user supplied TOML over the bundled table.
    pub fn with_override(mut self, content: &str) -> Result<Self, &'static str> {
        let override_table: OverrideTable =
            toml::from_str(content).map_err(|_| "覆盖价格表 TOML 格式错误")?;
        if override_table
            .currency
            .as_deref()
            .is_some_and(|value| value != "USD")
            || override_table
                .unit
                .as_deref()
                .is_some_and(|value| value != "per_million_tokens")
        {
            return Err("覆盖价格表必须使用 USD / per_million_tokens");
        }
        if override_table.models.values().any(|prices| !prices.valid())
            || override_table
                .subscriptions
                .as_ref()
                .is_some_and(|subscriptions| !subscriptions.valid())
            || override_table.server_tools.as_ref().is_some_and(|tools| {
                !tools.web_search_per_1k.is_finite() || tools.web_search_per_1k < 0.0
            })
        {
            return Err("覆盖价格表价格无效");
        }
        if let Some(version) = override_table.version {
            self.version = version;
        }
        if let Some(note) = override_table.note {
            self.note = note;
        }
        if let Some(sources) = override_table.sources {
            self.sources = sources;
        }
        self.models.extend(override_table.models);
        if let Some(subscriptions) = override_table.subscriptions {
            self.subscriptions.claude.extend(subscriptions.claude);
            self.subscriptions.chatgpt.extend(subscriptions.chatgpt);
        }
        if let Some(server_tools) = override_table.server_tools {
            self.server_tools = server_tools;
        }
        Ok(self)
    }

    #[must_use]
    pub fn subscription_usd(&self, tool: crate::model::Tool, plan: &str) -> Option<f64> {
        let normalized = plan.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        let key = match tool {
            crate::model::Tool::Claude => match normalized.as_str() {
                "pro" => "pro",
                "max_5x" | "max5x" => "max_5x",
                "max_20x" | "max20x" => "max_20x",
                _ => return None,
            },
            crate::model::Tool::Codex => match normalized.as_str() {
                "plus" => "plus",
                "pro" => "pro",
                _ => return None,
            },
        };
        match tool {
            crate::model::Tool::Claude => self.subscriptions.claude.get(key).copied(),
            crate::model::Tool::Codex => self.subscriptions.chatgpt.get(key).copied(),
        }
    }

    /// Exact match first, then longest model prefix followed by `-` or `@`.
    #[must_use]
    pub fn model_prices(&self, model: &str) -> Option<&ModelPrices> {
        if let Some(prices) = self.models.get(model) {
            return Some(prices);
        }
        self.models
            .iter()
            .filter(|(name, _)| {
                model
                    .strip_prefix(name.as_str())
                    .is_some_and(|suffix| suffix.starts_with('-') || suffix.starts_with('@'))
            })
            .max_by_key(|(name, _)| name.len())
            .map(|(_, prices)| prices)
    }

    #[must_use]
    pub fn cost_usd(&self, model: &str, tokens: &TokenCounts) -> Option<f64> {
        self.model_prices(model)
            .map(|prices| prices.cost_usd(tokens))
    }
}
