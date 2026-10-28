use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};

use super::{
    context_tokens, event_cost, format_tokens_lang, rebuild_cost, rebuild_tokens, AdviceInput, Hit,
    RuleOutput,
};
use crate::attribution::EventAttribution;
use crate::i18n::Lang;
use crate::model::{PromptEvent, Tool};

type SessionMap<'a> = BTreeMap<&'a str, Vec<&'a EventAttribution>>;

fn prompt_groups<'a>(
    tool: Tool,
    events: &[&'a EventAttribution],
    prompts: &[PromptEvent],
    since: DateTime<Utc>,
    include_subagents: bool,
) -> BTreeMap<(&'a str, DateTime<Utc>), Vec<&'a EventAttribution>> {
    let mut times: BTreeMap<&str, Vec<DateTime<Utc>>> = BTreeMap::new();
    for prompt in prompts.iter().filter(|prompt| prompt.tool == tool) {
        times
            .entry(&prompt.session_id)
            .or_default()
            .push(prompt.timestamp);
    }
    for values in times.values_mut() {
        values.sort();
        values.dedup();
    }
    let mut groups = BTreeMap::new();
    for item in events
        .iter()
        .filter(|item| include_subagents || !item.event.is_subagent)
    {
        let Some(starts) = times.get(item.event.session_id.as_str()) else {
            continue;
        };
        let Some(index) = starts
            .partition_point(|at| *at <= item.event.timestamp)
            .checked_sub(1)
        else {
            continue;
        };
        if starts[index] >= since {
            groups
                .entry((item.event.session_id.as_str(), starts[index]))
                .or_insert_with(Vec::new)
                .push(*item);
        }
    }
    groups
}

fn model_switch<'a>(
    tool: Tool,
    sessions: &SessionMap<'a>,
    since: DateTime<Utc>,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let config = &input.config.cache_rebuild;
    let mut hits = Vec::new();
    for events in sessions.values() {
        for pair in events.windows(2) {
            let previous = &pair[0].event;
            let current = &pair[1].event;
            if current.timestamp < since || previous.model == current.model {
                continue;
            }
            let ignored = input
                .config
                .model_switch
                .ignore_models
                .iter()
                .any(|pattern| {
                    previous
                        .model
                        .to_ascii_lowercase()
                        .contains(&pattern.to_ascii_lowercase())
                        || current
                            .model
                            .to_ascii_lowercase()
                            .contains(&pattern.to_ascii_lowercase())
                });
            if ignored {
                continue;
            }
            let ttl = match tool {
                Tool::Claude
                    if previous.tokens.cache_write_1h > 0 || current.tokens.cache_write_1h > 0 =>
                {
                    config.claude_ttl_1h_minutes
                }
                Tool::Claude => config.claude_ttl_5m_minutes,
                Tool::Codex => config.codex_ttl_minutes,
            };
            if current.timestamp - previous.timestamp > Duration::minutes(ttl) {
                continue;
            }
            let rebuilt = rebuild_tokens(current);
            let read = current.tokens.cache_read;
            if rebuilt < config.min_rebuild_tokens
                || read as f64 >= config.max_read_share * (read + rebuilt) as f64
            {
                continue;
            }
            let extra = input
                .pricing
                .model_prices(&current.model)
                .map_or(0.0, |prices| {
                    (rebuild_cost(current, prices)
                        - rebuilt as f64 * prices.cache_read / 1_000_000.0)
                        .max(0.0)
                });
            hits.push(Hit {
                item: pair[1],
                overhead_tokens: rebuilt,
                overhead_cost: extra,
                avoidable_cost: extra,
                actual_cost: None,
            });
        }
    }
    if hits.is_empty() {
        return None;
    }
    Some(RuleOutput {
        id: "model_switch",
        title: lang.pick("中途换模型导致缓存重建", "Cache rebuilt after a model switch").into(),
        facts: vec![
            crate::tr!(lang, "发生 {} 次", "{} occurrences", hits.len()),
            crate::tr!(lang, "累计重写 {} token", "{} tokens rewritten in total", format_tokens_lang(hits.iter().map(|hit| hit.overhead_tokens).sum(), lang)),
        ],
        description: lang.pick("同一会话中途换模型后，热缓存被重新写入。换模型可放在任务阶段之间，或换模型后开启新会话继续。", "Switching models within a session rewrote a warm cache. Switch between task stages, or continue in a new session after switching.").into(),
        occurrences: hits.len() as u64,
        hits,
        avoidable: true,
        tag: lang.pick("换模型重建缓存", "Model switch rebuilt cache"),
    })
}

fn context_spike<'a>(
    tool: Tool,
    sessions: &SessionMap<'a>,
    since: DateTime<Utc>,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let mut hits = Vec::new();
    let mut repeats = 0_u64;
    for events in sessions.values() {
        for (index, item) in events.iter().enumerate().skip(1) {
            let event = &item.event;
            if event.timestamp < since || events[index - 1].event.model != event.model {
                continue;
            }
            if input.compactions.iter().any(|compact| {
                compact.tool == tool
                    && compact.session_id == event.session_id
                    && compact.timestamp > events[index - 1].event.timestamp
                    && compact.timestamp <= event.timestamp
            }) {
                continue;
            }
            let read = event.tokens.cache_read;
            let written = rebuild_tokens(event);
            if (read as f64)
                < (1.0 - input.config.cache_rebuild.max_read_share) * (read + written) as f64
                || written < input.config.context_spike.min_spike_tokens
            {
                continue;
            }
            let next_compact = input
                .compactions
                .iter()
                .filter(|compact| {
                    compact.tool == tool
                        && compact.session_id == event.session_id
                        && compact.timestamp > event.timestamp
                })
                .map(|compact| compact.timestamp)
                .min();
            let count = events
                .iter()
                .skip(index + 1)
                .filter(|next| next_compact.is_none_or(|at| next.event.timestamp < at))
                .count() as u64;
            let excess = written.saturating_sub(input.config.context_spike.target_tokens);
            let price = input.pricing.model_prices(&event.model);
            let cost = price.map_or(0.0, |prices| {
                let write_per_token = if written == 0 {
                    0.0
                } else {
                    rebuild_cost(event, prices) * 1_000_000.0 / written as f64
                };
                excess as f64 * (write_per_token + prices.cache_read * count as f64) / 1_000_000.0
            });
            repeats += count;
            hits.push(Hit {
                item,
                overhead_tokens: excess,
                overhead_cost: cost,
                avoidable_cost: cost,
                actual_cost: None,
            });
        }
    }
    if hits.is_empty() {
        return None;
    }
    Some(RuleOutput {
        id: "context_spike",
        title: lang.pick("一次塞入大量内容", "Large context added at once").into(),
        facts: vec![
            crate::tr!(lang, "发生 {} 次", "{} occurrences", hits.len()),
            crate::tr!(lang, "后续重复读取 {repeats} 次", "Read again in {repeats} later requests"),
            crate::tr!(lang, "可缩减 {} token", "Potential reduction: {} tokens", format_tokens_lang(hits.iter().map(|hit| hit.overhead_tokens).sum(), lang)),
        ],
        description: lang.pick("大量内容会在后续请求中重复读取。日志先用 grep 或 tail 截取相关段落，大文件只读相关部分，大 diff 分批查看。", "Large inputs are read again in later requests. Use grep or tail to select relevant log sections, read only the parts of a file you need, and review large diffs in batches.").into(),
        occurrences: hits.len() as u64,
        hits,
        avoidable: true,
        tag: lang.pick("一次读入过多", "Large context increase"),
    })
}

fn runaway_prompt<'a>(
    tool: Tool,
    period: &[&'a EventAttribution],
    prompts: &[PromptEvent],
    since: DateTime<Utc>,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let groups = prompt_groups(tool, period, prompts, since, true);
    let mut counts = Vec::new();
    let mut hits = Vec::new();
    let mut prompts_hit = 0;
    let threshold = match tool {
        Tool::Claude => input.config.runaway_prompt.max_requests_per_prompt,
        Tool::Codex => input.config.runaway_prompt.codex_max_requests_per_prompt,
    };
    for events in groups
        .values()
        .filter(|events| events.len() as u64 >= threshold)
    {
        prompts_hit += 1;
        counts.push(events.len() as f64);
        for item in events {
            hits.push(Hit {
                item,
                overhead_tokens: item.event.tokens.total_without_reasoning(),
                overhead_cost: event_cost(input.pricing, &item.event),
                avoidable_cost: 0.0,
                actual_cost: None,
            });
        }
    }
    if hits.is_empty() {
        return None;
    }
    let average = counts.iter().sum::<f64>() / counts.len() as f64;
    let maximum = counts.into_iter().reduce(f64::max).unwrap_or(0.0);
    Some(RuleOutput {
        id: "runaway_prompt",
        title: lang.pick("一次提示跑了太多轮", "Many requests from one prompt").into(),
        facts: vec![
            crate::tr!(lang, "命中 {prompts_hit} 次提示", "{prompts_hit} matching prompts"),
            crate::tr!(lang, "平均每次提示 {average:.1} 次请求", "Average: {average:.1} requests per matching prompt"),
            crate::tr!(lang, "最多一次 {maximum:.0} 次", "Maximum: {maximum:.0} requests"),
        ],
        description:
            lang.pick("给出明确完成条件，先让工具列出计划；如果反复修同一个错误，可以打断并重新限定范围。", "Set clear completion criteria and ask for a plan first. If the same error is being fixed repeatedly, interrupt and narrow the scope.")
                .into(),
        occurrences: prompts_hit,
        hits,
        avoidable: false,
        tag: lang.pick("提示轮数过多", "Many requests per prompt"),
    })
}

fn expensive_subagent<'a>(
    tool: Tool,
    period: &[&'a EventAttribution],
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let (patterns, alternative) = match tool {
        Tool::Claude => (
            &input.config.light_task.claude_expensive,
            &input.config.expensive_subagent.claude_alternative,
        ),
        Tool::Codex => (
            &input.config.light_task.codex_expensive,
            &input.config.expensive_subagent.codex_alternative,
        ),
    };
    let alternative_prices = input.pricing.model_prices(alternative)?;
    let mut groups: SessionMap<'a> = BTreeMap::new();
    for item in period.iter().filter(|item| {
        item.event.is_subagent
            && patterns.iter().any(|pattern| {
                item.event
                    .model
                    .to_ascii_lowercase()
                    .contains(&pattern.to_ascii_lowercase())
            })
    }) {
        groups.entry(&item.event.session_id).or_default().push(item);
    }
    let mut hits = Vec::new();
    let mut sessions = 0;
    for events in groups
        .values()
        .filter(|events| events.len() as u64 >= input.config.expensive_subagent.min_requests)
    {
        sessions += 1;
        for item in events {
            let cost = (event_cost(input.pricing, &item.event)
                - alternative_prices.cost_usd(&item.event.tokens))
            .max(0.0);
            hits.push(Hit {
                item,
                overhead_tokens: item.event.tokens.total_without_reasoning(),
                overhead_cost: cost,
                avoidable_cost: cost,
                actual_cost: None,
            });
        }
    }
    if hits.is_empty() {
        return None;
    }
    Some(RuleOutput {
        id: "expensive_subagent",
        title: lang
            .pick("子代理用了高价模型", "Expensive models used by subagents")
            .into(),
        facts: vec![
            crate::tr!(lang, "涉及 {sessions} 个会话", "Across {sessions} sessions"),
            crate::tr!(
                lang,
                "高价模型请求 {} 次",
                "{} requests on expensive models",
                hits.len()
            ),
        ],
        description: match tool {
            Tool::Claude => lang.pick(
                "简单子任务可在子代理定义里写 model: sonnet。",
                "For simple subtasks, set model: sonnet in the subagent definition.",
            ),
            Tool::Codex => lang.pick(
                "简单子任务可选用较低价的模型。",
                "Consider a lower-cost model for simple subtasks.",
            ),
        }
        .into(),
        occurrences: hits.len() as u64,
        hits,
        avoidable: true,
        tag: lang.pick("子代理高价模型", "Expensive subagent model"),
    })
}

fn heavy_reasoning<'a>(
    tool: Tool,
    period: &[&'a EventAttribution],
    prompts: &[PromptEvent],
    since: DateTime<Utc>,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let mut hits = Vec::new();
    let mut prompts_hit = 0;
    for events in prompt_groups(tool, period, prompts, since, true).values() {
        let reasoning: u64 = events.iter().map(|item| item.event.tokens.reasoning).sum();
        let visible: u64 = events
            .iter()
            .map(|item| {
                item.event
                    .tokens
                    .output
                    .saturating_sub(item.event.tokens.reasoning)
            })
            .sum();
        if reasoning < input.config.heavy_reasoning.min_reasoning_tokens
            || visible >= input.config.light_task.max_output_tokens
            || reasoning as f64 / ((reasoning + visible).max(1) as f64)
                < input.config.heavy_reasoning.min_reasoning_share
        {
            continue;
        }
        prompts_hit += 1;
        for item in events {
            let cost = input
                .pricing
                .model_prices(&item.event.model)
                .map_or(0.0, |price| {
                    item.event.tokens.reasoning as f64
                        * (1.0 - input.config.heavy_reasoning.keep_share)
                        * price.output
                        / 1_000_000.0
                });
            hits.push(Hit {
                item,
                overhead_tokens: item.event.tokens.reasoning,
                overhead_cost: cost,
                avoidable_cost: cost,
                actual_cost: None,
            });
        }
    }
    if hits.is_empty() {
        return None;
    }
    Some(RuleOutput {
        id: "heavy_reasoning",
        title: lang.pick("推理 token 占比过高", "High reasoning token share").into(),
        facts: vec![
            crate::tr!(lang, "命中 {prompts_hit} 次提示", "{prompts_hit} matching prompts"),
            crate::tr!(lang, "推理 {} token", "{} reasoning tokens",
                format_tokens_lang(hits.iter().map(|hit| hit.overhead_tokens).sum(), lang)
            ),
        ],
        description: lang.pick("简单任务可通过 Codex 的 model_reasoning_effort 或 /model 调低推理强度。", "For simple tasks, reduce reasoning effort with Codex model_reasoning_effort or /model.")
            .into(),
        occurrences: prompts_hit,
        hits,
        avoidable: true,
        tag: lang.pick("推理占比过高", "High reasoning share"),
    })
}

fn fast_mode<'a>(
    tool: Tool,
    period: &[&'a EventAttribution],
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    if tool != Tool::Claude {
        return None;
    }
    let multiplier = input.config.fast_mode.claude_price_multiplier;
    let hits: Vec<_> = period
        .iter()
        .filter(|item| item.event.fast)
        .map(|item| {
            let priced_model =
                input.config.fast_mode.claude_models.iter().any(|model| {
                    item.event.model == *model
                        || item.event.model.strip_prefix(model).is_some_and(|suffix| {
                            suffix.starts_with('-') || suffix.starts_with('@')
                        })
                });
            let standard_cost = event_cost(input.pricing, &item.event);
            let actual_cost = if multiplier > 0.0 && priced_model {
                Some(standard_cost * multiplier)
            } else {
                None
            };
            let extra = actual_cost.map_or(0.0, |_| standard_cost * (multiplier - 1.0).max(0.0));
            Hit {
                item,
                overhead_tokens: item.event.tokens.total_without_reasoning(),
                overhead_cost: extra,
                avoidable_cost: extra,
                actual_cost,
            }
        })
        .collect();
    if hits.is_empty() {
        return None;
    }
    let avoidable = hits.iter().any(|hit| hit.avoidable_cost > 0.0);
    Some(RuleOutput {
        id: "fast_mode",
        title: lang.pick("使用了快速模式", "Fast mode enabled").into(),
        facts: vec![
            crate::tr!(lang, "快速请求 {} 次", "{} fast-mode requests", hits.len()),
            crate::tr!(lang, "合计 {} token", "{} tokens in total",
                format_tokens_lang(hits.iter().map(|hit| hit.overhead_tokens).sum(), lang)
            ),
        ],
        description: if multiplier == 0.0 {
            lang.pick("不赶时间时可以用 /fast 关闭快速模式。价格倍率尚待核实，当前不估算溢价。", "Turn off fast mode with /fast when speed is not essential. The price multiplier is unverified, so no premium is estimated.")
        } else if !avoidable {
            lang.pick("不赶时间时可以用 /fast 关闭快速模式。当前请求模型没有已核实的快速模式倍率，因此不估算溢价。", "Turn off fast mode with /fast when speed is not essential. This model has no verified fast-mode multiplier, so no premium is estimated.")
        } else {
            lang.pick("不赶时间时可以用 /fast 关闭快速模式；额外费用按配置倍率估算。", "Turn off fast mode with /fast when speed is not essential. Extra cost is estimated using the configured multiplier.")
        }
        .into(),
        occurrences: hits.len() as u64,
        hits,
        avoidable,
        tag: lang.pick("快速模式", "Fast mode"),
    })
}

fn high_effort_light_task<'a>(
    tool: Tool,
    period: &[&'a EventAttribution],
    prompts: &[PromptEvent],
    since: DateTime<Utc>,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let efforts = match tool {
        Tool::Claude => &input.config.high_effort_light_task.claude_high_efforts,
        Tool::Codex => &input.config.high_effort_light_task.codex_high_efforts,
    };
    let mut hits = Vec::new();
    let mut prompts_hit = 0;
    for events in prompt_groups(tool, period, prompts, since, false).values() {
        let output: u64 = events.iter().map(|item| item.event.tokens.output).sum();
        if events.len() as u64 > input.config.light_task.max_requests
            || output >= input.config.light_task.max_output_tokens
            || !events.iter().all(|item| {
                item.event.effort.as_ref().is_some_and(|effort| {
                    efforts
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(effort))
                })
            })
        {
            continue;
        }
        prompts_hit += 1;
        for item in events {
            hits.push(Hit {
                item,
                overhead_tokens: item.event.tokens.total_without_reasoning(),
                overhead_cost: event_cost(input.pricing, &item.event),
                avoidable_cost: 0.0,
                actual_cost: None,
            });
        }
    }
    if hits.is_empty() {
        return None;
    }
    Some(RuleOutput {
        id: "high_effort_light_task",
        title: lang
            .pick(
                "轻量任务用了高推理强度",
                "High reasoning effort on light tasks",
            )
            .into(),
        facts: vec![
            crate::tr!(
                lang,
                "命中 {prompts_hit} 次提示",
                "{prompts_hit} matching prompts"
            ),
            crate::tr!(lang, "涉及 {} 次请求", "Across {} requests", hits.len()),
        ],
        description: match tool {
            Tool::Claude => lang.pick(
                "轻量任务可通过 /effort 或 /model 调到 high、medium。",
                "For light tasks, use /effort or /model to select high or medium effort.",
            ),
            Tool::Codex => lang.pick(
                "轻量任务可通过 model_reasoning_effort 或 /model 调低推理强度。",
                "For light tasks, reduce effort with model_reasoning_effort or /model.",
            ),
        }
        .into(),
        occurrences: prompts_hit,
        hits,
        avoidable: false,
        tag: lang.pick("轻量任务高推理", "High effort on a light task"),
    })
}

fn frequent_compaction<'a>(
    tool: Tool,
    period: &[&'a EventAttribution],
    since: DateTime<Utc>,
    now: DateTime<Utc>,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let mut groups = BTreeMap::<&str, Vec<_>>::new();
    for compact in input.compactions.iter().filter(|compact| {
        compact.tool == tool && compact.timestamp >= since && compact.timestamp <= now
    }) {
        groups.entry(&compact.session_id).or_default().push(compact);
    }
    let mut hits = Vec::new();
    let mut sessions = 0;
    let mut auto = 0;
    let mut manual = 0;
    let mut unknown = 0;
    let mut pre_sum = 0_u64;
    let mut pre_count = 0_u64;
    for (session, compactions) in groups.into_iter().filter(|(_, events)| {
        events.len() as u64 >= input.config.frequent_compaction.min_compactions
    }) {
        sessions += 1;
        for compact in compactions {
            match compact.trigger.as_deref() {
                Some("manual") => manual += 1,
                Some("auto") => auto += 1,
                _ => unknown += 1,
            }
            let item = period
                .iter()
                .filter(|item| {
                    item.event.session_id == session && item.event.timestamp <= compact.timestamp
                })
                .max_by_key(|item| item.event.timestamp)
                .or_else(|| period.iter().find(|item| item.event.session_id == session));
            let Some(item) = item else {
                continue;
            };
            let tokens = compact
                .pre_tokens
                .unwrap_or_else(|| context_tokens(&item.event.tokens));
            pre_sum = pre_sum.saturating_add(tokens);
            pre_count += 1;
            let cost = input
                .pricing
                .model_prices(&item.event.model)
                .map_or(0.0, |price| tokens as f64 * price.cache_read / 1_000_000.0);
            hits.push(Hit {
                item,
                overhead_tokens: tokens,
                overhead_cost: cost,
                avoidable_cost: 0.0,
                actual_cost: None,
            });
        }
    }
    if sessions == 0 {
        return None;
    }
    Some(RuleOutput {
        id: "frequent_compaction",
        title: lang.pick("频繁压缩上下文", "Frequent context compaction").into(),
        facts: vec![
            crate::tr!(lang, "涉及 {sessions} 个会话", "Across {sessions} sessions"),
            crate::tr!(lang, "自动 {auto} 次、手动 {manual} 次、未标注 {unknown} 次", "{auto} automatic, {manual} manual, {unknown} unspecified"),
            crate::tr!(lang, "平均压缩前 {} token", "Average before compaction: {} tokens",
                format_tokens_lang(pre_sum / pre_count.max(1), lang)
            ),
        ],
        description: lang.pick("一个会话只做一件事，阶段完成后可以开启新会话，避免反复压缩同一段上下文。", "Keep one task per session. Start a new session after a task stage to avoid compacting the same context repeatedly.")
            .into(),
        occurrences: auto + manual + unknown,
        hits,
        avoidable: false,
        tag: lang.pick("频繁压缩", "Frequent compaction"),
    })
}

fn web_search<'a>(
    tool: Tool,
    period: &[&'a EventAttribution],
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    if tool != Tool::Claude {
        return None;
    }
    let searches: u64 = period
        .iter()
        .map(|item| u64::from(item.event.web_search_requests))
        .sum();
    if searches < input.config.web_search.min_searches {
        return None;
    }
    let hits = period
        .iter()
        .filter(|item| item.event.web_search_requests > 0)
        .map(|item| {
            let count = u64::from(item.event.web_search_requests);
            Hit {
                item,
                overhead_tokens: 0,
                overhead_cost: count as f64 * input.pricing.server_tools.web_search_per_1k / 1000.0,
                avoidable_cost: 0.0,
                actual_cost: None,
            }
        })
        .collect();
    Some(RuleOutput {
        id: "web_search",
        title: lang.pick("联网搜索次数较多", "Frequent web searches").into(),
        facts: vec![crate::tr!(lang, "联网搜索 {searches} 次", "{searches} web searches")],
        description: if input.pricing.server_tools.web_search_per_1k == 0.0 {
            lang.pick("已知网址可以直接提供；文档类问题可优先查询本地文件。搜索单价尚待核实。", "Provide known URLs directly and check local files first for documentation questions. The search unit price is unverified.")
        } else {
            lang.pick("已知网址可以直接提供；文档类问题可优先查询本地文件。搜索费用按配置单价估算。", "Provide known URLs directly and check local files first for documentation questions. Search cost is estimated at the configured rate.")
        }
        .into(),
        occurrences: searches,
        hits,
        avoidable: false,
        tag: lang.pick("联网搜索较多", "Frequent web searches"),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn usage_rules<'a>(
    tool: Tool,
    sessions: &SessionMap<'a>,
    period: &[&'a EventAttribution],
    prompts: &[PromptEvent],
    since: DateTime<Utc>,
    now: DateTime<Utc>,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Vec<RuleOutput<'a>> {
    [
        model_switch(tool, sessions, since, input, lang),
        context_spike(tool, sessions, since, input, lang),
        runaway_prompt(tool, period, prompts, since, input, lang),
        expensive_subagent(tool, period, input, lang),
        heavy_reasoning(tool, period, prompts, since, input, lang),
        fast_mode(tool, period, input, lang),
        high_effort_light_task(tool, period, prompts, since, input, lang),
        frequent_compaction(tool, period, since, now, input, lang),
        web_search(tool, period, input, lang),
    ]
    .into_iter()
    .flatten()
    .collect()
}

pub(super) fn quota_rules<'a>(
    tool: Tool,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Vec<RuleOutput<'a>> {
    [
        quota_rebalance(tool, input, lang),
        plan_fit(tool, input, lang),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn quota_rebalance<'a>(tool: Tool, input: &AdviceInput<'_>, lang: Lang) -> Option<RuleOutput<'a>> {
    let busy = input.quotas.iter().find(|state| state.tool == Some(tool))?;
    let idle = input.quotas.iter().find(|state| {
        state.tool
            == Some(if tool == Tool::Claude {
                Tool::Codex
            } else {
                Tool::Claude
            })
    })?;
    let busy_pct = busy.weekly_pct?;
    let idle_pct = idle.weekly_pct?;
    if busy_pct < input.config.quota_rebalance.busy_weekly_pct
        || idle_pct > input.config.quota_rebalance.idle_weekly_pct
        || !idle.used_recently
        || !busy
            .predicted_hit_at
            .is_some_and(|hit| busy.resets_at.is_some_and(|reset| hit < reset))
    {
        return None;
    }
    Some(RuleOutput {
        id: "quota_rebalance",
        title: lang.pick("两个额度池可以分流", "Use the other tool's available quota").into(),
        facts: vec![
            crate::tr!(lang, "当前周额度已用 {busy_pct:.0}%", "Current tool: {busy_pct:.0}% of weekly quota used"),
            crate::tr!(lang, "另一工具已用 {idle_pct:.0}%", "Other tool: {idle_pct:.0}% of weekly quota used"),
        ],
        description: lang.pick("当前工具预计在重置前触顶，另一工具近 30 天有使用且额度较空。适合的任务可以分流到另一工具。", "The current tool is forecast to hit its limit before reset. The other tool was used in the last 30 days and has more quota available. Consider moving suitable tasks to it.").into(),
        occurrences: 1,
        hits: Vec::new(),
        avoidable: false,
        tag: lang.pick("额度池分流", "Quota balancing"),
    })
}

fn plan_fit<'a>(tool: Tool, input: &AdviceInput<'_>, lang: Lang) -> Option<RuleOutput<'a>> {
    let state = input.quotas.iter().find(|state| state.tool == Some(tool))?;
    let config = &input.config.plan_fit;
    if state.completed_weekly_max.len() < config.min_weeks {
        return None;
    }
    let plan = state
        .plan
        .as_deref()?
        .to_ascii_lowercase()
        .replace([' ', '-'], "_");
    let capacities = match tool {
        Tool::Claude => &config.claude_capacity,
        Tool::Codex => &config.chatgpt_capacity,
    };
    let current_capacity = *capacities.get(&plan)?;
    let current_price = input.pricing.subscription_usd(tool, &plan)?;
    let weeks = &state.completed_weekly_max[..config.min_weeks];
    let cheaper = capacities
        .iter()
        .filter_map(|(candidate, capacity)| {
            let price = input.pricing.subscription_usd(tool, candidate)?;
            (price < current_price
                && *capacity > 0.0
                && weeks
                    .iter()
                    .all(|max| max * current_capacity / capacity <= config.headroom * 100.0))
            .then_some((candidate, price))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let capped = weeks.iter().filter(|max| **max >= 100.0).count();
    let result = if let Some((candidate, price)) = cheaper {
        (true, candidate.as_str(), current_price - price)
    } else if capped >= config.min_capped_weeks {
        let next = capacities
            .iter()
            .filter_map(|(candidate, capacity)| {
                let price = input.pricing.subscription_usd(tool, candidate)?;
                (price > current_price && *capacity > current_capacity)
                    .then_some((candidate, price))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((candidate, price)) = next {
            (false, candidate.as_str(), price - current_price)
        } else if tool == Tool::Codex && plan == "plus" {
            let price = input.pricing.subscription_usd(tool, "pro")?;
            if price <= current_price {
                return None;
            }
            (false, "pro", price - current_price)
        } else {
            return None;
        }
    } else {
        return None;
    };
    Some(RuleOutput {
        id: "plan_fit",
        title: lang.pick("套餐是否合适", "Check your plan fit").into(),
        facts: vec![
            crate::tr!(lang, "最近 {} 个完整周最高用量：{}", "Peak usage over the last {} complete weeks: {}",
                config.min_weeks,
                weeks
                    .iter()
                    .map(|max| format!("{max:.0}%"))
                    .collect::<Vec<_>>()
                    .join(lang.pick("、", ", "))
            ),
            crate::tr!(lang, "{}到 {}，月费{}约 ${:.0}", "{} to {}; monthly fee {} by about ${:.0}",
                if result.0 { lang.pick("降档", "Downgrade") } else { lang.pick("升档", "Upgrade") },
                plan_display_name(result.1),
                if result.0 {
                    lang.pick("减少", "decreases")
                } else {
                    lang.pick("增加", "increases")
                },
                result.2
            ),
        ],
        description:
            lang.pick("根据已记录的周额度最高值与公开月费估算。实际套餐容量、税费和地区价格可能不同。", "Estimated from recorded weekly peaks and published monthly fees. Actual plan capacity, taxes and regional prices may differ.").into(),
        occurrences: 1,
        hits: Vec::new(),
        avoidable: false,
        tag: lang.pick("套餐评估", "Plan fit"),
    })
}

fn plan_display_name(plan: &str) -> &str {
    match plan {
        "pro" => "Pro",
        "max_5x" => "Max 5x",
        "max_20x" => "Max 20x",
        "plus" => "Plus",
        _ => plan,
    }
}
