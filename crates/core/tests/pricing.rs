use token_audit_core::model::TokenCounts;
use token_audit_core::model::Tool;
use token_audit_core::pricing::Pricing;

#[test]
fn bundled_table_parses_and_charges_all_fields_except_reasoning() {
    let pricing = Pricing::bundled().expect("bundled prices");
    assert_eq!(pricing.version, "2026-10-24");
    assert_eq!(pricing.currency, "USD");
    assert_eq!(pricing.unit, "per_million_tokens");
    assert_eq!(pricing.sources.len(), 2);
    assert_eq!(pricing.subscription_usd(Tool::Claude, "pro"), Some(20.0));
    assert_eq!(
        pricing.subscription_usd(Tool::Claude, "max_5x"),
        Some(100.0)
    );
    assert_eq!(
        pricing.subscription_usd(Tool::Claude, "max-20x"),
        Some(200.0)
    );
    assert_eq!(pricing.subscription_usd(Tool::Codex, "plus"), Some(20.0));
    assert_eq!(pricing.subscription_usd(Tool::Codex, "pro"), Some(200.0));
    assert_eq!(pricing.subscription_usd(Tool::Codex, "team"), None);
    let tokens = TokenCounts {
        input: 1_000_000,
        output: 2_000_000,
        cache_read: 500_000,
        cache_write_5m: 100_000,
        cache_write_1h: 200_000,
        cache_write_unspecified: 300_000,
        reasoning: 9_000_000,
    };
    // 4 + 40 + 0.1 + 0.5 + 1.6 + 1.5 = 47.7.
    let cost = pricing
        .cost_usd("claude-opus-5-5", &tokens)
        .expect("priced model");
    assert!((cost - 47.7).abs() < 1e-9);
}

#[test]
fn override_replaces_whole_model_and_adds_new_model() {
    let pricing = Pricing::bundled()
        .expect("bundled prices")
        .with_override(
            r#"
version = "custom-1"
note = "自定义估算"
[models."claude-opus-5-5"]
input = 9
output = 8
cache_read = 7
cache_write_5m = 6
cache_write_1h = 5
cache_write_unspecified = 4
[models."custom-model"]
input = 1
output = 2
cache_read = 3
cache_write_5m = 4
cache_write_1h = 5
cache_write_unspecified = 6
"#,
        )
        .expect("override prices");
    assert_eq!(pricing.version, "custom-1");
    assert_eq!(pricing.note, "自定义估算");
    let one_million = TokenCounts {
        input: 1_000_000,
        ..TokenCounts::default()
    };
    assert_eq!(pricing.cost_usd("claude-opus-5-5", &one_million), Some(9.0));
    assert_eq!(pricing.cost_usd("custom-model", &one_million), Some(1.0));
    assert_eq!(pricing.cost_usd("claude-sonnet-5", &one_million), Some(2.0));
}

#[test]
fn longest_boundary_prefix_and_unknown_model() {
    let pricing = Pricing::bundled().expect("bundled prices");
    let one_million = TokenCounts {
        input: 1_000_000,
        ..TokenCounts::default()
    };
    assert_eq!(
        pricing.cost_usd("claude-opus-4-8-20260101", &one_million),
        Some(5.0)
    );
    assert_eq!(
        pricing.cost_usd("gpt-6-sol@variant", &one_million),
        Some(2.0)
    );
    assert_eq!(pricing.cost_usd("gpt-6-solx", &one_million), None);
    assert_eq!(pricing.cost_usd("gpt-6", &one_million), None);
    assert_eq!(pricing.cost_usd("unknown-model", &one_million), None);

    let pricing = pricing
        .with_override(
            "[models.\"gpt-6\"]\ninput = 99\noutput = 99\ncache_read = 99\ncache_write_5m = 99\ncache_write_1h = 99\ncache_write_unspecified = 99",
        )
        .expect("add generic model");
    assert_eq!(pricing.cost_usd("gpt-6-sol", &one_million), Some(2.0));
    assert_eq!(
        pricing.cost_usd("gpt-6-sol-20261024", &one_million),
        Some(2.0)
    );
}

#[test]
fn malformed_or_partial_override_fails_without_echoing_content() {
    let pricing = Pricing::bundled().expect("bundled prices");
    let error = pricing
        .clone()
        .with_override("private prompt = [")
        .unwrap_err();
    assert_eq!(error, "覆盖价格表 TOML 格式错误");
    assert!(pricing.with_override("[models.x]\ninput = 1").is_err());
    assert!(Pricing::bundled()
        .expect("bundled prices")
        .with_override("currency = 'EUR'")
        .is_err());
}

#[test]
fn subscription_override_preserves_other_plans_and_rejects_invalid_prices() {
    let pricing = Pricing::bundled()
        .expect("bundled prices")
        .with_override("[subscriptions.claude]\npro = 25")
        .expect("subscription override");
    assert_eq!(pricing.subscription_usd(Tool::Claude, "pro"), Some(25.0));
    assert_eq!(
        pricing.subscription_usd(Tool::Claude, "max_5x"),
        Some(100.0)
    );
    assert_eq!(pricing.subscription_usd(Tool::Codex, "plus"), Some(20.0));
    assert!(pricing
        .with_override("[subscriptions.chatgpt]\nplus = -1")
        .is_err());
}
