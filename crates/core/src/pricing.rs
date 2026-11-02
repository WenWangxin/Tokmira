use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Timelike, Utc};
use serde::Deserialize;

use crate::model::{TokenCounts, Tool, UsageEvent};

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
    #[serde(default)]
    note_en: String,
    sources: Vec<PricingSource>,
    models: BTreeMap<String, ModelPrices>,
    subscriptions: Subscriptions,
    server_tools: ServerTools,
    deepseek: DeepSeekPricing,
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
    note_en: Option<String>,
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
    pub note_en: String,
    pub sources: Vec<PricingSource>,
    models: BTreeMap<String, ModelPrices>,
    subscriptions: Subscriptions,
    pub server_tools: ServerTools,
    pub deepseek: DeepSeekPricing,
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
            || !table.deepseek.valid()
        {
            return Err("内置价格表价格无效");
        }
        Ok(Self {
            version: table.version,
            currency: table.currency,
            unit: table.unit,
            note: table.note,
            note_en: table.note_en,
            sources: table.sources,
            models: table.models,
            subscriptions: table.subscriptions,
            server_tools: table.server_tools,
            deepseek: table.deepseek,
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
        if let Some(note) = override_table.note_en {
            self.note_en = note;
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
            crate::model::Tool::DeepSeek => return None,
        };
        match tool {
            crate::model::Tool::Claude => self.subscriptions.claude.get(key).copied(),
            crate::model::Tool::Codex => self.subscriptions.chatgpt.get(key).copied(),
            crate::model::Tool::DeepSeek => None,
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

    /// USD cost of one logged request; DeepSeek Harness bills in yuan, so it never has one.
    #[must_use]
    pub fn event_cost_usd(&self, event: &UsageEvent) -> Option<f64> {
        (event.tool != Tool::DeepSeek)
            .then(|| self.cost_usd(&event.model, &event.tokens))
            .flatten()
    }
}

/// DeepSeek's official yuan price list, with its weekday peak hours in Beijing time.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DeepSeekPricing {
    pub currency: String,
    pub source: String,
    utc_offset_hours: i32,
    /// `[start, end)` hours on Monday to Friday, Beijing time.
    peak_hours: Vec<[u32; 2]>,
    off_peak_ratio: f64,
    /// Beijing dates (`YYYY-MM-DD`) on a weekday that are billed off-peak.
    #[serde(default)]
    holidays: Vec<NaiveDate>,
    /// Title requests carry no usage; their size in bytes is divided by this.
    pub title_bytes_per_token: f64,
    pub title_output_tokens: u64,
    models: BTreeMap<String, DeepSeekModelPrices>,
}

/// Peak-hour yuan per million tokens. Cache writes are billed as cache misses.
#[derive(Clone, Debug, Deserialize)]
pub struct DeepSeekModelPrices {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl DeepSeekPricing {
    fn valid(&self) -> bool {
        let price = |value: f64| value.is_finite() && value >= 0.0;
        !self.models.is_empty()
            && self.currency == "CNY"
            && (-12..=14).contains(&self.utc_offset_hours)
            && self
                .peak_hours
                .iter()
                .all(|[start, end]| start < end && *end <= 24)
            && price(self.off_peak_ratio)
            && self.title_bytes_per_token.is_finite()
            && self.title_bytes_per_token > 0.0
            && self.models.values().all(|prices| {
                price(prices.input) && price(prices.output) && price(prices.cache_read)
            })
    }

    fn model(&self, model: &str) -> Option<&DeepSeekModelPrices> {
        self.models.get(model).or_else(|| {
            self.models
                .values()
                .find(|prices| prices.aliases.iter().any(|alias| alias == model))
        })
    }

    /// Whether `at` falls in DeepSeek's peak hours. Weekday holidays are only
    /// known when listed, so an unlisted one is billed here at the peak price.
    #[must_use]
    pub fn is_peak(&self, at: DateTime<Utc>) -> bool {
        let Some(offset) = FixedOffset::east_opt(self.utc_offset_hours * 3600) else {
            return false;
        };
        let local = at.with_timezone(&offset);
        if local.weekday().number_from_monday() > 5 || self.holidays.contains(&local.date_naive()) {
            return false;
        }
        let hour = local.hour();
        self.peak_hours
            .iter()
            .any(|[start, end]| (*start..*end).contains(&hour))
    }

    /// Yuan for one request at the price in force when it was made.
    #[must_use]
    pub fn cost_cny(&self, model: &str, tokens: &TokenCounts, at: DateTime<Utc>) -> Option<f64> {
        let prices = self.model(model)?;
        let ratio = if self.is_peak(at) {
            1.0
        } else {
            self.off_peak_ratio
        };
        let input = tokens
            .input
            .saturating_add(tokens.cache_write_5m)
            .saturating_add(tokens.cache_write_1h)
            .saturating_add(tokens.cache_write_unspecified);
        Some(
            (input as f64 * prices.input
                + tokens.output as f64 * prices.output
                + tokens.cache_read as f64 * prices.cache_read)
                * ratio
                / 1_000_000.0,
        )
    }

    /// Estimated tokens for a title request of `bytes`, which logs no usage.
    #[must_use]
    pub fn title_tokens(&self, bytes: u64) -> TokenCounts {
        TokenCounts {
            input: (bytes as f64 / self.title_bytes_per_token).ceil() as u64,
            output: self.title_output_tokens,
            ..TokenCounts::default()
        }
    }
}
