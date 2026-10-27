//! Pure text formatting for the tray; all quota values come from the shared overview.

use chrono::{DateTime, Datelike, Local, Utc};
use token_audit_core::dashboard::{QuotaDisplay, ToolOverview};
use token_audit_core::forecast::Forecast;
use token_audit_core::settings::MenuBarQuota;

pub struct MenuText {
    pub title: Option<String>,
    pub tooltip: String,
    pub lines: Vec<String>,
    pub needs_authorization: bool,
}

fn passed(display: &QuotaDisplay, now: DateTime<Utc>) -> bool {
    display.window_passed || display.resets_at.is_some_and(|at| at <= now)
}

fn percent(display: &QuotaDisplay, now: DateTime<Utc>) -> Option<String> {
    let value = if passed(display, now) {
        Some(0.0)
    } else {
        display.used_percent
    }?;
    let urgent = !passed(display, now)
        && matches!(display.forecast, Forecast::WillHit { at } if at >= now && at - now <= chrono::Duration::hours(1));
    Some(format!(
        "{}%{}",
        value.round() as u32,
        if urgent { "!" } else { "" }
    ))
}

fn reset_time(at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(at) = at else {
        return "重置时间未知".to_owned();
    };
    let local = at.with_timezone(&Local);
    if local.date_naive() == now.with_timezone(&Local).date_naive() {
        local.format("%H:%M 重置").to_string()
    } else {
        format!("{}/{} 重置", local.month(), local.day())
    }
}

fn tool_lines(
    name: &str,
    tool: Option<&ToolOverview>,
    enabled: bool,
    now: DateTime<Utc>,
) -> Vec<String> {
    if !enabled {
        return vec![format!("{name} 未开启额度来源")];
    }
    let Some(tool) = tool else {
        return vec![format!("{name} 暂无额度数据")];
    };
    let mut lines = Vec::new();
    let reset = |display: &QuotaDisplay| {
        if passed(display, now) {
            "已重置".to_owned()
        } else {
            reset_time(display.resets_at, now)
        }
    };
    if let Some(value) = percent(&tool.five_hour, now) {
        lines.push(format!(
            "{name}   5 小时 {value} · {}",
            reset(&tool.five_hour)
        ));
    }
    if let Some(value) = percent(&tool.weekly, now) {
        lines.push(format!("         本周 {value} · {}", reset(&tool.weekly)));
    }
    if let Forecast::WillHit { at } = tool.five_hour.forecast {
        if at > now && !passed(&tool.five_hour, now) {
            let minutes = (at - now).num_minutes().max(1);
            lines.push(format!("         约 {minutes} 分钟后达到上限"));
        }
    }
    if lines.is_empty() {
        lines.push(format!("{name} 暂无额度数据"));
    }
    lines
}

#[expect(
    clippy::too_many_arguments,
    reason = "all inputs are explicit snapshot values"
)]
pub fn format_menu(
    claude: Option<&ToolOverview>,
    codex: Option<&ToolOverview>,
    claude_on: bool,
    codex_on: bool,
    needs_authorization: bool,
    updated_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    mode: MenuBarQuota,
) -> MenuText {
    let cl = claude_on
        .then_some(claude)
        .flatten()
        .and_then(|v| percent(&v.five_hour, now));
    let cx = codex_on
        .then_some(codex)
        .flatten()
        .and_then(|v| percent(&v.five_hour, now));
    let selected = match mode {
        MenuBarQuota::Off => Vec::new(),
        MenuBarQuota::Claude => cl.as_ref().map(|v| vec![("Cl", v)]).unwrap_or_default(),
        MenuBarQuota::Codex => cx.as_ref().map(|v| vec![("Cx", v)]).unwrap_or_default(),
        MenuBarQuota::Both => {
            let mut list = Vec::new();
            if let Some(v) = &cl {
                list.push(("Cl", v));
            }
            if let Some(v) = &cx {
                list.push(("Cx", v));
            }
            list
        }
    };
    let title = if selected.is_empty() {
        None
    } else if selected.len() == 1 {
        Some(selected[0].1.to_string())
    } else {
        Some(
            selected
                .iter()
                .map(|(name, value)| format!("{name} {value}"))
                .collect::<Vec<_>>()
                .join(" · "),
        )
    };
    let mut tooltip_parts = vec!["token-audit".to_owned()];
    if let Some(v) = &cl {
        tooltip_parts.push(format!("Claude {v}"));
    }
    if let Some(v) = &cx {
        tooltip_parts.push(format!("Codex {v}"));
    }
    let mut lines = tool_lines("Claude", claude, claude_on, now);
    lines.extend(tool_lines("Codex", codex, codex_on, now));
    if lines.iter().all(|line| line.contains("暂无额度数据")) {
        lines = vec!["暂无额度数据".to_owned()];
    }
    lines.push(format!(
        "更新于 {}",
        updated_at
            .map(|at| at.with_timezone(&Local).format("%H:%M").to_string())
            .unwrap_or_else(|| "--:--".to_owned())
    ));
    MenuText {
        title,
        tooltip: tooltip_parts.join(" · "),
        lines,
        needs_authorization: claude_on && needs_authorization,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use token_audit_core::model::Tool;
    fn empty(tool: Tool) -> ToolOverview {
        let display = QuotaDisplay {
            used_percent: None,
            resets_at: None,
            origin: None,
            forecast: Forecast::Unknown,
            window_passed: false,
        };
        ToolOverview {
            tool,
            plan: None,
            subscription_usd: None,
            five_hour: display.clone(),
            weekly: display,
        }
    }
    #[test]
    fn empty_disabled_and_authorization() {
        let now = Utc::now();
        let empty_text = format_menu(None, None, true, true, false, None, now, MenuBarQuota::Both);
        assert_eq!(empty_text.lines[0], "暂无额度数据");
        let disabled = format_menu(
            None,
            None,
            false,
            false,
            true,
            None,
            now,
            MenuBarQuota::Both,
        );
        assert!(disabled.lines[0].contains("未开启"));
        assert!(!disabled.needs_authorization);
        assert!(
            format_menu(None, None, true, false, true, None, now, MenuBarQuota::Off)
                .needs_authorization
        );
    }
    #[test]
    fn selection_reset_and_forecast() {
        let now = Utc::now();
        let mut cl = empty(Tool::Claude);
        cl.five_hour.used_percent = Some(82.0);
        cl.five_hour.forecast = Forecast::WillHit {
            at: now + chrono::Duration::minutes(7),
        };
        cl.five_hour.resets_at = Some(now + chrono::Duration::minutes(30));
        cl.weekly.used_percent = Some(70.0);
        let mut cx = empty(Tool::Codex);
        cx.five_hour.used_percent = Some(40.0);
        cx.five_hour.window_passed = true;
        assert_eq!(
            format_menu(
                Some(&cl),
                Some(&cx),
                true,
                true,
                false,
                None,
                now,
                MenuBarQuota::Both
            )
            .title
            .as_deref(),
            Some("Cl 82%! · Cx 0%")
        );
        assert_eq!(
            format_menu(
                Some(&cl),
                Some(&cx),
                true,
                true,
                false,
                None,
                now,
                MenuBarQuota::Claude
            )
            .title
            .as_deref(),
            Some("82%!")
        );
        assert_eq!(
            format_menu(
                Some(&cl),
                Some(&cx),
                true,
                true,
                false,
                None,
                now,
                MenuBarQuota::Codex
            )
            .title
            .as_deref(),
            Some("0%")
        );
        assert!(format_menu(
            Some(&cl),
            Some(&cx),
            true,
            true,
            false,
            None,
            now,
            MenuBarQuota::Off
        )
        .title
        .is_none());
        assert!(format_menu(
            Some(&cl),
            None,
            true,
            false,
            false,
            None,
            now,
            MenuBarQuota::Both
        )
        .lines
        .iter()
        .any(|line| line.contains("7 分钟")));
        let text = format_menu(
            Some(&cl),
            Some(&cx),
            true,
            true,
            false,
            None,
            now,
            MenuBarQuota::Both,
        );
        assert!(
            text.lines[0].contains(&format!(
                "{} 重置",
                (now + chrono::Duration::minutes(30))
                    .with_timezone(&Local)
                    .format("%H:%M")
            )) || text.lines[0].contains("/")
        );
        assert!(text.tooltip.contains("Claude 82%!"));
        assert!(text
            .lines
            .iter()
            .any(|line| line == "Codex   5 小时 0% · 已重置"));
    }

    #[test]
    fn a_reset_window_drops_its_forecast() {
        let now = Utc::now();
        let mut cl = empty(Tool::Claude);
        cl.five_hour.used_percent = Some(97.0);
        cl.five_hour.resets_at = Some(now - chrono::Duration::minutes(1));
        cl.five_hour.forecast = Forecast::WillHit {
            at: now + chrono::Duration::minutes(5),
        };
        let text = format_menu(
            Some(&cl),
            None,
            true,
            false,
            false,
            None,
            now,
            MenuBarQuota::Claude,
        );
        assert_eq!(text.title.as_deref(), Some("0%"));
        assert_eq!(text.lines[0], "Claude   5 小时 0% · 已重置");
        assert!(!text.lines.iter().any(|line| line.contains("达到上限")));
    }
}
