//! Pure text formatting for the tray; all quota values come from the shared overview.

use chrono::{DateTime, Datelike, Local, Utc};
use token_audit_core::dashboard::{QuotaDisplay, ToolOverview};
use token_audit_core::forecast::Forecast;
use token_audit_core::model::Tool;

pub struct MenuText {
    /// Logo and 5-hour percent next to the menu bar icon, top to bottom; empty shows
    /// the icon alone.
    pub badges: Vec<(Tool, String)>,
    pub tooltip: String,
    pub sections: Vec<ToolSection>,
    pub updated: String,
    pub needs_authorization: bool,
}

/// One tool's block in the menu, Claude first: a heading with its logo, then plain rows
/// under it.
pub struct ToolSection {
    pub heading: String,
    pub rows: Vec<String>,
}

pub fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Claude => "Claude",
        Tool::Codex => "Codex",
    }
}

/// The plan after the tool name, named as on the dashboard; unknown plans show as stored.
fn plan_label(tool: Tool, plan: Option<&str>) -> Option<String> {
    let plan = plan.map(str::trim).filter(|plan| !plan.is_empty())?;
    let key = plan.to_lowercase().replace([' ', '-'], "_");
    let known = match (tool, key.as_str()) {
        (Tool::Claude, "pro") | (Tool::Codex, "pro") => "Pro",
        (Tool::Claude, "max") => "Max",
        (Tool::Claude, "max_5x") => "Max 5x",
        (Tool::Claude, "max_20x") => "Max 20x",
        (Tool::Codex, "plus") => "Plus",
        _ => return Some(plan.to_owned()),
    };
    Some(known.to_owned())
}

fn heading(tool: Tool, overview: Option<&ToolOverview>) -> String {
    match plan_label(tool, overview.and_then(|value| value.plan.as_deref())) {
        Some(plan) => format!("{} · {plan}", tool_name(tool)),
        None => tool_name(tool).to_owned(),
    }
}

fn passed(display: &QuotaDisplay, now: DateTime<Utc>) -> bool {
    display.window_passed || display.resets_at.is_some_and(|at| at <= now)
}

/// `urgent_mark` adds "!" when the window is expected to run out within the hour; the
/// menu leaves it off because its forecast row already says so.
fn percent(display: &QuotaDisplay, now: DateTime<Utc>, urgent_mark: bool) -> Option<String> {
    let value = if passed(display, now) {
        Some(0.0)
    } else {
        display.used_percent
    }?;
    let urgent = urgent_mark
        && !passed(display, now)
        && matches!(display.forecast, Forecast::WillHit { at } if at >= now && at - now <= chrono::Duration::hours(1));
    // "≈" marks a number that still includes requests not yet confirmed by a reading.
    let estimated = !passed(display, now) && display.estimated_percent.is_some();
    Some(format!(
        "{}{}%{}",
        if estimated { "≈" } else { "" },
        value.round() as u32,
        if urgent { "!" } else { "" }
    ))
}

fn reset_time(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let local = at.with_timezone(&Local);
    if local.date_naive() == now.with_timezone(&Local).date_naive() {
        local.format("%H:%M 重置").to_string()
    } else {
        format!("{}/{} 重置", local.month(), local.day())
    }
}

fn duration(minutes: i64) -> String {
    if minutes < 60 {
        format!("{minutes} 分钟")
    } else if minutes % 60 == 0 {
        format!("{} 小时", minutes / 60)
    } else {
        format!("{} 小时 {} 分", minutes / 60, minutes % 60)
    }
}

fn window_row(label: &str, display: &QuotaDisplay, now: DateTime<Utc>) -> Option<String> {
    if passed(display, now) && display.used_percent.is_some() {
        return Some(format!("{label}额度已重置"));
    }
    let value = percent(display, now, false)?;
    Some(match display.resets_at {
        Some(at) => format!("{label}已用 {value} · {}", reset_time(at, now)),
        None => format!("{label}已用 {value}"),
    })
}

fn tool_rows(tool: Option<&ToolOverview>, enabled: bool, now: DateTime<Utc>) -> Vec<String> {
    if !enabled {
        return vec!["未开启额度来源".to_owned()];
    }
    let Some(tool) = tool else {
        return vec!["暂无额度数据".to_owned()];
    };
    let mut rows = Vec::new();
    rows.extend(window_row("5 小时", &tool.five_hour, now));
    rows.extend(window_row("本周", &tool.weekly, now));
    if let Forecast::WillHit { at } = tool.five_hour.forecast {
        if at > now && !passed(&tool.five_hour, now) {
            let minutes = (at - now).num_minutes().max(1);
            rows.push(format!("按当前速度约 {}后用完", duration(minutes)));
        }
    }
    if rows.is_empty() {
        rows.push("暂无额度数据".to_owned());
    }
    rows
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
    shown: &[Tool],
) -> MenuText {
    let tools = [
        (Tool::Claude, claude, claude_on),
        (Tool::Codex, codex, codex_on),
    ];
    let current = |tool: Option<&ToolOverview>, on: bool| {
        on.then_some(tool)
            .flatten()
            .and_then(|v| percent(&v.five_hour, now, true))
    };
    // A fixed order keeps each logo on the same row however the selection was made.
    let badges = tools
        .iter()
        .filter(|(tool, _, _)| shown.contains(tool))
        .filter_map(|(tool, overview, on)| current(*overview, *on).map(|value| (*tool, value)))
        .collect();
    let mut tooltip_parts = vec!["token-audit".to_owned()];
    for (tool, overview, on) in &tools {
        if let Some(value) = current(*overview, *on) {
            tooltip_parts.push(format!("{} {value}", tool_name(*tool)));
        }
    }
    let sections = tools
        .iter()
        .map(|(tool, overview, on)| ToolSection {
            heading: heading(*tool, *overview),
            rows: tool_rows(*overview, *on, now),
        })
        .collect();
    MenuText {
        badges,
        tooltip: tooltip_parts.join(" · "),
        sections,
        updated: format!(
            "更新于 {}",
            updated_at
                .map(|at| at.with_timezone(&Local).format("%H:%M").to_string())
                .unwrap_or_else(|| "--:--".to_owned())
        ),
        needs_authorization: claude_on && needs_authorization,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn empty(tool: Tool) -> ToolOverview {
        let display = QuotaDisplay {
            used_percent: None,
            resets_at: None,
            origin: None,
            forecast: Forecast::Unknown,
            window_passed: false,
            as_of: None,
            estimated_percent: None,
        };
        ToolOverview {
            tool,
            plan: None,
            subscription_usd: None,
            five_hour: display.clone(),
            weekly: display,
        }
    }
    fn badges(text: &MenuText) -> Vec<(Tool, &str)> {
        text.badges
            .iter()
            .map(|(tool, value)| (*tool, value.as_str()))
            .collect()
    }
    const BOTH: &[Tool] = &[Tool::Codex, Tool::Claude];

    #[test]
    fn empty_disabled_and_authorization() {
        let now = Utc::now();
        let empty_text = format_menu(None, None, true, true, false, None, now, BOTH);
        assert_eq!(empty_text.sections[0].rows, ["暂无额度数据"]);
        assert_eq!(empty_text.updated, "更新于 --:--");
        let disabled = format_menu(None, None, false, false, true, None, now, BOTH);
        assert_eq!(disabled.sections[1].rows, ["未开启额度来源"]);
        assert!(!disabled.needs_authorization);
        assert!(format_menu(None, None, true, false, true, None, now, &[]).needs_authorization);
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
        let menu =
            |shown: &[Tool]| format_menu(Some(&cl), Some(&cx), true, true, false, None, now, shown);
        // Claude stays on top even when Codex was picked first.
        assert_eq!(
            badges(&menu(BOTH)),
            [(Tool::Claude, "82%!"), (Tool::Codex, "0%")]
        );
        assert_eq!(badges(&menu(&[Tool::Claude])), [(Tool::Claude, "82%!")]);
        assert_eq!(badges(&menu(&[Tool::Codex])), [(Tool::Codex, "0%")]);
        assert!(menu(&[]).badges.is_empty());
        // A tool without data drops out of the menu bar.
        assert!(format_menu(
            Some(&cl),
            None,
            true,
            false,
            false,
            None,
            now,
            &[Tool::Codex]
        )
        .badges
        .is_empty());
        let text = menu(BOTH);
        assert_eq!(
            text.sections[0].rows,
            [
                format!(
                    "5 小时已用 82% · {}",
                    reset_time(now + chrono::Duration::minutes(30), now)
                ),
                "本周已用 70%".to_owned(),
                "按当前速度约 7 分钟后用完".to_owned(),
            ]
        );
        assert_eq!(text.sections[1].rows, ["5 小时额度已重置"]);
        assert!(text.tooltip.contains("Claude 82%!"));
    }

    #[test]
    fn headings_name_the_plan() {
        let mut cl = empty(Tool::Claude);
        cl.plan = Some("max_5x".to_owned());
        let mut cx = empty(Tool::Codex);
        cx.plan = Some(" ".to_owned());
        let text = format_menu(
            Some(&cl),
            Some(&cx),
            true,
            true,
            false,
            None,
            Utc::now(),
            &[],
        );
        assert_eq!(text.sections[0].heading, "Claude · Max 5x");
        assert_eq!(text.sections[1].heading, "Codex");
        assert_eq!(
            plan_label(Tool::Codex, Some("team")).as_deref(),
            Some("team")
        );
    }

    #[test]
    fn long_forecasts_read_in_hours() {
        assert_eq!(duration(7), "7 分钟");
        assert_eq!(duration(120), "2 小时");
        assert_eq!(duration(219), "3 小时 39 分");
    }

    #[test]
    fn an_estimated_percent_is_marked() {
        let now = Utc::now();
        let mut cl = empty(Tool::Claude);
        cl.five_hour.used_percent = Some(63.4);
        cl.five_hour.estimated_percent = Some(2.4);
        cl.five_hour.resets_at = Some(now + chrono::Duration::hours(2));
        let text = format_menu(
            Some(&cl),
            None,
            true,
            false,
            false,
            None,
            now,
            &[Tool::Claude],
        );
        assert_eq!(badges(&text), [(Tool::Claude, "≈63%")]);
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
            &[Tool::Claude],
        );
        assert_eq!(badges(&text), [(Tool::Claude, "0%")]);
        assert_eq!(text.sections[0].rows, ["5 小时额度已重置"]);
    }
}
