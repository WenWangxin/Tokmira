import type { ShareSummary } from '../shareLogic'

// M8B_DEV_PREVIEW_ONLY: handwritten figures matching the share-card design.
export function mockShareSummary(): ShareSummary {
  return {
    year: 2026,
    month: 10,
    month_estimated_cost_usd: 187.4,
    tools: [
      { tool: 'Claude', plan: 'pro', subscription_usd: 20, estimated_cost_usd: 121.8 },
      { tool: 'Codex', plan: 'plus', subscription_usd: 20, estimated_cost_usd: 65.6 },
    ],
    recognized_subscriptions: 2,
    subscription_total_usd: 40,
    subscription_value_multiple: 4.685,
    unrecognized_tools: [],
    cache_hit_rate: 0.68,
    requests: 1284,
    top_project: { name: 'token-audit', estimated_cost_usd: 65.6 },
  }
}
