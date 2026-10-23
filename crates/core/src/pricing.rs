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
}

#[derive(Clone, Debug)]
pub struct Pricing {
    pub version: String,
    pub currency: String,
    pub unit: String,
    pub note: String,
    pub sources: Vec<PricingSource>,
    models: BTreeMap<String, ModelPrices>,
}

impl Pricing {
    /// Parses the bundled, offline price table.
    pub fn bundled() -> Result<Self, &'static str> {
        let table: DefaultTable = toml::from_str(include_str!("../../../config/pricing.toml"))
            .map_err(|_| "内置价格表格式错误")?;
        if table.models.is_empty() || table.models.values().any(|prices| !prices.valid()) {
            return Err("内置价格表价格无效");
        }
        Ok(Self {
            version: table.version,
            currency: table.currency,
            unit: table.unit,
            note: table.note,
            sources: table.sources,
            models: table.models,
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
        if override_table.models.values().any(|prices| !prices.valid()) {
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
        Ok(self)
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
