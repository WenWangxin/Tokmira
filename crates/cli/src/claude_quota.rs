use std::ffi::OsString;
use std::time::Duration;

use chrono::Local;
use token_audit_core::claude_quota::{
    query_claude_quota, system_proxy, BreakdownRow, ClaudeQuota, ClaudeQuotaError,
    CredentialPreference, QueryOptions, SystemSecretStore, UreqTransport,
};

struct Args {
    source: CredentialPreference,
    proxy: Option<String>,
    no_proxy: bool,
    timeout: Duration,
    keychain_timeout: Duration,
    json: bool,
}

fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Args, String> {
    let mut result = Args {
        source: CredentialPreference::Auto,
        proxy: None,
        no_proxy: false,
        timeout: Duration::from_secs(15),
        keychain_timeout: Duration::from_secs(60),
        json: false,
    };
    let mut source_set = false;
    let mut timeout_set = false;
    let mut keychain_timeout_set = false;
    let mut args = args.peekable();
    while let Some(flag) = args.next() {
        match flag.to_str() {
            Some("--json") if !result.json => result.json = true,
            Some("--no-proxy") if !result.no_proxy => result.no_proxy = true,
            Some("--source") if !source_set => {
                let value = args.next().ok_or("--source 缺少参数")?;
                result.source = match value.to_str() {
                    Some("auto") => CredentialPreference::Auto,
                    Some("desktop") => CredentialPreference::ClaudeDesktop,
                    Some("code") => CredentialPreference::ClaudeCode,
                    _ => return Err("--source 只能是 auto、desktop 或 code".into()),
                };
                source_set = true;
            }
            Some("--proxy") if result.proxy.is_none() => {
                let value = args.next().ok_or("--proxy 缺少参数")?;
                let value = value.to_str().ok_or("--proxy 不是有效文本")?;
                if !value.starts_with("http://") || value.len() <= "http://".len() {
                    return Err("--proxy 应为 http://host:port".into());
                }
                result.proxy = Some(value.to_owned());
            }
            Some("--timeout") if !timeout_set => {
                let value = args.next().ok_or("--timeout 缺少参数")?;
                let seconds = value
                    .to_str()
                    .and_then(|value| value.parse::<u64>().ok())
                    .filter(|seconds| *seconds > 0)
                    .ok_or("--timeout 应为大于 0 的整数秒")?;
                result.timeout = Duration::from_secs(seconds);
                timeout_set = true;
            }
            Some("--keychain-timeout") if !keychain_timeout_set => {
                let value = args.next().ok_or("--keychain-timeout 缺少参数")?;
                let seconds = value
                    .to_str()
                    .and_then(|value| value.parse::<u64>().ok())
                    .filter(|seconds| *seconds > 0)
                    .ok_or("--keychain-timeout 应为大于 0 的整数秒")?;
                result.keychain_timeout = Duration::from_secs(seconds);
                keychain_timeout_set = true;
            }
            _ => return Err("未知或重复的 Claude 额度参数".into()),
        }
    }
    Ok(result)
}

fn resolve_proxy(args: &Args) -> Option<String> {
    if args.no_proxy {
        return None;
    }
    args.proxy
        .clone()
        .or_else(|| {
            ["HTTPS_PROXY", "https_proxy", "ALL_PROXY"]
                .into_iter()
                .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        })
        .or_else(system_proxy)
}

fn format_breakdown_row(row: &BreakdownRow) -> String {
    let name = match row.key.as_str() {
        "claude_code" => "Claude Code",
        "chat" => "聊天",
        "cowork" => "Cowork",
        "other" => "其他",
        _ => row.display_name.as_deref().unwrap_or(&row.key),
    };
    format!("  {name} {}%", row.percent)
}

fn print_quota(quota: &ClaudeQuota) {
    println!("Claude 官方额度");
    println!("来源          {}", quota.source.label());
    println!(
        "套餐          {}",
        quota.subscription_type.as_deref().unwrap_or("未知")
    );
    if let Some(reason) = &quota.fallback_reason {
        println!("退回原因      {reason}");
    }
    for window in &quota.windows {
        let reset = window
            .resets_at
            .map(|value| {
                value
                    .with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|| "未知".to_owned());
        println!(
            "{}    已用 {:.1}%    重置：{reset}",
            window.name, window.utilization
        );
    }
    if let Some(breakdown) = &quota.weekly_breakdown {
        println!("本周占比（官方）");
        for row in &breakdown.rows {
            println!("{}", format_breakdown_row(row));
        }
        println!("本机日志只覆盖 Claude Code，聊天和 Cowork 的用量来自官方占比");
    }
    if let Some(extra) = &quota.extra_usage {
        println!(
            "额外用量      {}",
            match extra.is_enabled {
                Some(true) => "已开启",
                Some(false) => "未开启",
                None => "未知",
            }
        );
        if let Some(value) = extra.monthly_limit {
            println!("  月度上限    {value}");
        }
        if let Some(value) = extra.used_credits {
            println!("  已用额度    {value}");
        }
        if let Some(value) = extra.utilization {
            println!("  已用比例    {value:.1}%");
        }
    }
}

pub fn run(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let args = parse_args(args)?;
    let home = dirs::home_dir().ok_or("无法确定用户主目录")?;
    let label = match args.source {
        CredentialPreference::Auto => "Claude 桌面版或 Claude Code",
        CredentialPreference::ClaudeDesktop => "Claude 桌面版",
        CredentialPreference::ClaudeCode => "Claude Code",
    };
    let notice =
        format!("将读取{label}的登录令牌，仅向 api.anthropic.com 查询额度，令牌不会被保存");
    if args.json {
        eprintln!("{notice}");
    } else {
        println!("{notice}");
    }
    #[cfg(target_os = "macos")]
    if args.source != CredentialPreference::ClaudeCode {
        eprintln!("如果弹出钥匙串授权框，请选择‘允许’（选‘始终允许’以后不再询问）");
    }
    let options = QueryOptions {
        preference: args.source,
        proxy: resolve_proxy(&args),
        timeout: args.timeout,
        keychain_timeout: args.keychain_timeout,
    };
    let quota = query_claude_quota(&SystemSecretStore::default(), &home, &UreqTransport, &options).map_err(|error| match error {
        ClaudeQuotaError::TokenExpired { source } => format!("{}的登录令牌已过期。请打开一次 Claude 桌面版 / 在终端运行一次 claude，让它自己刷新后再试；Tokmira 不会自行刷新令牌。", source.label()),
        other => other.to_string(),
    })?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&quota).map_err(|_| "生成 JSON 失败")?
        );
    } else {
        print_quota(&quota);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{format_breakdown_row, parse_args};
    use std::ffi::OsString;
    use token_audit_core::claude_quota::{BreakdownRow, CredentialPreference};

    #[test]
    fn formats_official_weekly_breakdown_rows() {
        for (key, display_name, expected) in [
            ("claude_code", None, "  Claude Code 98%"),
            ("chat", Some("Chats"), "  聊天 98%"),
            ("cowork", None, "  Cowork 98%"),
            ("other", None, "  其他 98%"),
            ("new_surface", Some("New Surface"), "  New Surface 98%"),
            ("unknown", None, "  unknown 98%"),
        ] {
            assert_eq!(
                format_breakdown_row(&BreakdownRow {
                    key: key.into(),
                    display_name: display_name.map(str::to_owned),
                    percent: 98.0,
                }),
                expected
            );
        }
    }

    #[test]
    fn parses_claude_quota_options() {
        let args = [
            "--source",
            "code",
            "--no-proxy",
            "--timeout",
            "3",
            "--keychain-timeout",
            "2",
            "--json",
        ];
        let result = parse_args(args.into_iter().map(OsString::from)).expect("valid args");
        assert_eq!(result.source, CredentialPreference::ClaudeCode);
        assert!(result.no_proxy && result.json);
        assert_eq!(result.timeout.as_secs(), 3);
        assert_eq!(result.keychain_timeout.as_secs(), 2);
        let both = parse_args(
            ["--proxy", "http://local:123", "--no-proxy"]
                .into_iter()
                .map(OsString::from),
        )
        .expect("no proxy overrides explicit proxy");
        assert!(both.no_proxy);
        let defaults = parse_args(std::iter::empty()).expect("default args");
        assert_eq!(defaults.keychain_timeout.as_secs(), 60);
        assert!(parse_args(["--keychain-timeout", "0"].into_iter().map(OsString::from)).is_err());
    }
}
