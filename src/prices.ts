import type { CostEstimate, PriceSetting, Provider, SessionListItem } from "./types";

export const defaultPrices: PriceSetting[] = [
  price("codex", "gpt-5.6-sol", "GPT-5.6 Sol", 4, 0.4, 5, 20, "https://developers.openai.com/api/docs/models/gpt-5.6-sol"),
  price("codex", "gpt-5.6-terra", "GPT-5.6 Terra", 2, 0.2, 2.5, 12, "https://developers.openai.com/api/docs/models/gpt-5.6-terra"),
  price("codex", "gpt-5.6-luna", "GPT-5.6 Luna", 0.2, 0.02, 0.25, 1.2, "https://developers.openai.com/api/docs/models/gpt-5.6-luna"),
  price("codex", "gpt-5.5", "GPT-5.5", 5, 0.5, undefined, 30, "https://developers.openai.com/api/docs/models/gpt-5.5"),
  price("claude", "sonnet-5", "Claude Sonnet 5", 3, 0.3, 3.75, 15, "https://platform.claude.com/docs/en/about-claude/pricing"),
  price("claude", "opus-5", "Claude Opus 5", 5, 0.5, 6.25, 25, "https://platform.claude.com/docs/en/about-claude/pricing"),
  price("claude", "sonnet-4.6", "Claude Sonnet 4.6", 3, 0.3, 3.75, 15, "https://platform.claude.com/docs/en/about-claude/pricing"),
  price("claude", "opus-4.", "Claude Opus 4.x", 5, 0.5, 6.25, 25, "https://platform.claude.com/docs/en/about-claude/pricing"),
  price("claude", "haiku-4.5", "Claude Haiku 4.5", 1, 0.1, 1.25, 5, "https://platform.claude.com/docs/en/about-claude/pricing"),
  price("grok", "grok-4.6", "Grok 4.6", 2, 0.5, undefined, 6, "https://docs.x.ai/developers/models/grok-4.6"),
  price("grok", "grok-build-0.1", "Grok Build 0.1", 1, 0.2, undefined, 2, "https://docs.x.ai/developers/pricing"),
  price("antigravity", "gemini-3.7-flash", "Gemini 3.7 Flash", 0.75, 0.075, undefined, 3.75, "https://ai.google.dev/gemini-api/docs/pricing"),
];

function price(provider: Provider, pattern: string, label: string, input: number, cached: number, cacheWrite: number | undefined, output: number, source: string): PriceSetting {
  return {
    id: `${provider}:${pattern}`,
    provider,
    model_pattern: pattern,
    catalog_model: label,
    input_per_million_usd: input,
    cached_input_per_million_usd: cached,
    cache_write_per_million_usd: cacheWrite,
    output_per_million_usd: output,
    effective_date: "2026-09-03",
    source_url: source,
    built_in: true,
  };
}

export function loadPrices(): PriceSetting[] {
  try {
    const saved = JSON.parse(localStorage.getItem("contextractor.prices.v2") || "[]") as PriceSetting[];
    const byId = new Map(defaultPrices.map((entry) => [entry.id, entry]));
    saved.forEach((entry) => byId.set(entry.id, entry));
    return [...byId.values()];
  } catch {
    return defaultPrices;
  }
}

export function savePrices(prices: PriceSetting[]) {
  localStorage.setItem("contextractor.prices.v2", JSON.stringify(prices));
}

export function withUnknownModels(prices: PriceSetting[], sessions: SessionListItem[]): PriceSetting[] {
  const result = [...prices];
  for (const session of sessions) {
    if (!session.model) continue;
    const normalized = session.model.toLowerCase();
    const matched = result.some((entry) => entry.provider === session.provider && normalized.includes(entry.model_pattern.toLowerCase()));
    if (!matched && !result.some((entry) => entry.id === `${session.provider}:${normalized}`)) {
      result.push({
        id: `${session.provider}:${normalized}`,
        provider: session.provider,
        model_pattern: session.model,
        catalog_model: session.model,
        built_in: false,
      });
    }
  }
  return result;
}

export function applyPriceOverride(cost: CostEstimate, provider: Provider, prices: PriceSetting[]): CostEstimate {
  const model = cost.model?.toLowerCase();
  if (!model) return cost;
  const entry = prices.find((item) => item.provider === provider && model.includes(item.model_pattern.toLowerCase()));
  if (!entry || entry.input_per_million_usd == null || entry.output_per_million_usd == null) return cost;
  const uncached = Math.max(0, cost.input_tokens - cost.cached_input_tokens);
  const cachedRate = entry.cached_input_per_million_usd ?? entry.input_per_million_usd;
  const cacheWriteRate = entry.cache_write_per_million_usd ?? entry.input_per_million_usd;
  const normalizedUncached = provider === "claude" ? cost.input_tokens : Math.max(0, uncached - cost.cache_write_input_tokens);
  return {
    ...cost,
    catalog_model: entry.catalog_model,
    amount_usd: (normalizedUncached * entry.input_per_million_usd + cost.cached_input_tokens * cachedRate + cost.cache_write_input_tokens * cacheWriteRate + cost.output_tokens * entry.output_per_million_usd) / 1_000_000,
    pricing_date: entry.effective_date,
    source_url: entry.source_url,
    note: `${entry.built_in ? "Tarihli katalog" : "Kullanıcı fiyatı"}; kaydedilmiş cache okuma/yazma dahildir. Tool ücretleri, cache saklama ve uzun-context çarpanları dahil değildir.`,
  };
}
