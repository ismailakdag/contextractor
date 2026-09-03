use crate::model::{Role, StoredSession, UsageConfidence};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPrice {
    pub provider: &'static str,
    pub model_pattern: &'static str,
    pub catalog_model: &'static str,
    pub input_per_million_usd: f64,
    pub cached_input_per_million_usd: Option<f64>,
    pub cache_write_per_million_usd: Option<f64>,
    pub output_per_million_usd: f64,
    pub effective_date: &'static str,
    pub source_url: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    pub model: Option<String>,
    pub catalog_model: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_write_input_tokens: i64,
    pub reasoning_tokens: i64,
    pub amount_usd: Option<f64>,
    pub confidence: UsageConfidence,
    pub pricing_date: Option<String>,
    pub source_url: Option<String>,
    pub note: String,
}

pub const PRICE_CATALOG: &[ModelPrice] = &[
    ModelPrice {
        provider: "codex",
        model_pattern: "gpt-5.6-sol",
        catalog_model: "GPT-5.6 Sol",
        input_per_million_usd: 4.0,
        cached_input_per_million_usd: Some(0.4),
        cache_write_per_million_usd: Some(5.0),
        output_per_million_usd: 20.0,
        effective_date: "2026-09-02",
        source_url: "https://developers.openai.com/api/docs/models/gpt-5.6-sol",
    },
    ModelPrice {
        provider: "codex",
        model_pattern: "gpt-5.6-terra",
        catalog_model: "GPT-5.6 Terra",
        input_per_million_usd: 2.0,
        cached_input_per_million_usd: Some(0.2),
        cache_write_per_million_usd: Some(2.5),
        output_per_million_usd: 12.0,
        effective_date: "2026-09-02",
        source_url: "https://developers.openai.com/api/docs/models/gpt-5.6-terra",
    },
    ModelPrice {
        provider: "codex",
        model_pattern: "gpt-5.6-luna",
        catalog_model: "GPT-5.6 Luna",
        input_per_million_usd: 0.2,
        cached_input_per_million_usd: Some(0.02),
        cache_write_per_million_usd: Some(0.25),
        output_per_million_usd: 1.2,
        effective_date: "2026-09-02",
        source_url: "https://developers.openai.com/api/docs/models/gpt-5.6-luna",
    },
    ModelPrice {
        provider: "codex",
        model_pattern: "gpt-5.5",
        catalog_model: "GPT-5.5",
        input_per_million_usd: 5.0,
        cached_input_per_million_usd: Some(0.5),
        cache_write_per_million_usd: None,
        output_per_million_usd: 30.0,
        effective_date: "2026-09-02",
        source_url: "https://developers.openai.com/api/docs/models/gpt-5.5",
    },
    ModelPrice {
        provider: "claude",
        model_pattern: "sonnet-5",
        catalog_model: "Claude Sonnet 5",
        input_per_million_usd: 3.0,
        cached_input_per_million_usd: Some(0.3),
        cache_write_per_million_usd: Some(3.75),
        output_per_million_usd: 15.0,
        effective_date: "2026-09-01",
        source_url: "https://platform.claude.com/docs/en/about-claude/pricing",
    },
    ModelPrice {
        provider: "claude",
        model_pattern: "opus-5",
        catalog_model: "Claude Opus 5",
        input_per_million_usd: 5.0,
        cached_input_per_million_usd: Some(0.5),
        cache_write_per_million_usd: Some(6.25),
        output_per_million_usd: 25.0,
        effective_date: "2026-09-02",
        source_url: "https://platform.claude.com/docs/en/about-claude/pricing",
    },
    ModelPrice {
        provider: "claude",
        model_pattern: "sonnet-4.6",
        catalog_model: "Claude Sonnet 4.6",
        input_per_million_usd: 3.0,
        cached_input_per_million_usd: Some(0.3),
        cache_write_per_million_usd: Some(3.75),
        output_per_million_usd: 15.0,
        effective_date: "2026-09-02",
        source_url: "https://platform.claude.com/docs/en/about-claude/pricing",
    },
    ModelPrice {
        provider: "claude",
        model_pattern: "opus-4.",
        catalog_model: "Claude Opus 4.x",
        input_per_million_usd: 5.0,
        cached_input_per_million_usd: Some(0.5),
        cache_write_per_million_usd: Some(6.25),
        output_per_million_usd: 25.0,
        effective_date: "2026-09-02",
        source_url: "https://platform.claude.com/docs/en/about-claude/pricing",
    },
    ModelPrice {
        provider: "claude",
        model_pattern: "haiku-4.5",
        catalog_model: "Claude Haiku 4.5",
        input_per_million_usd: 1.0,
        cached_input_per_million_usd: Some(0.1),
        cache_write_per_million_usd: Some(1.25),
        output_per_million_usd: 5.0,
        effective_date: "2026-09-02",
        source_url: "https://platform.claude.com/docs/en/about-claude/pricing",
    },
    ModelPrice {
        provider: "grok",
        model_pattern: "grok-4.6",
        catalog_model: "Grok 4.6",
        input_per_million_usd: 2.0,
        cached_input_per_million_usd: Some(0.5),
        cache_write_per_million_usd: None,
        output_per_million_usd: 6.0,
        effective_date: "2026-09-02",
        source_url: "https://docs.x.ai/developers/models/grok-4.6",
    },
    ModelPrice {
        provider: "grok",
        model_pattern: "grok-build-0.1",
        catalog_model: "Grok Build 0.1",
        input_per_million_usd: 1.0,
        cached_input_per_million_usd: Some(0.2),
        cache_write_per_million_usd: None,
        output_per_million_usd: 2.0,
        effective_date: "2026-09-02",
        source_url: "https://docs.x.ai/developers/pricing",
    },
    ModelPrice {
        provider: "antigravity",
        model_pattern: "gemini-3.7-flash",
        catalog_model: "Gemini 3.7 Flash",
        input_per_million_usd: 0.75,
        cached_input_per_million_usd: Some(0.075),
        cache_write_per_million_usd: None,
        output_per_million_usd: 3.75,
        effective_date: "2026-09-02",
        source_url: "https://ai.google.dev/gemini-api/docs/pricing",
    },
];

pub fn estimate_session_cost(session: &StoredSession) -> CostEstimate {
    let mut input_tokens = 0_i64;
    let mut output_tokens = 0_i64;
    let mut cached_input_tokens = 0_i64;
    let mut cache_write_input_tokens = 0_i64;
    let mut reasoning_tokens = 0_i64;
    let mut observed = false;

    for turn in &session.turns {
        if let Some(usage) = &turn.usage {
            if usage.confidence == Some(UsageConfidence::Observed) {
                observed = true;
            }
            input_tokens += usage.input_tokens.unwrap_or(0);
            output_tokens += usage.output_tokens.unwrap_or(0);
            cached_input_tokens += usage.cached_input_tokens.unwrap_or(0);
            cache_write_input_tokens += usage.cache_write_input_tokens.unwrap_or(0);
            reasoning_tokens += usage.reasoning_tokens.unwrap_or(0);
        }
    }

    let confidence = if observed && (input_tokens > 0 || output_tokens > 0) {
        UsageConfidence::Observed
    } else {
        input_tokens = session
            .turns
            .iter()
            .filter(|turn| matches!(turn.role, Role::User | Role::System | Role::Tool))
            .map(|turn| approximate_tokens(&turn.text) + tool_tokens(turn))
            .sum();
        output_tokens = session
            .turns
            .iter()
            .filter(|turn| matches!(turn.role, Role::Assistant | Role::Reasoning))
            .map(|turn| approximate_tokens(&turn.text))
            .sum();
        UsageConfidence::Estimated
    };

    estimate_usage_cost(
        &session.session.provider,
        session.session.model.clone(),
        crate::model::TokenUsage {
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            cached_input_tokens: Some(cached_input_tokens),
            cache_write_input_tokens: Some(cache_write_input_tokens),
            reasoning_tokens: Some(reasoning_tokens),
            total_tokens: Some(input_tokens + output_tokens),
            confidence: Some(confidence),
            source: Some(if confidence == UsageConfidence::Estimated {
                "character approximation".to_string()
            } else {
                "provider record".to_string()
            }),
        },
    )
}

pub fn estimate_usage_cost(
    provider: &str,
    model: Option<String>,
    usage: crate::model::TokenUsage,
) -> CostEstimate {
    let price = model.as_deref().and_then(|model| {
        let normalized = model.to_ascii_lowercase();
        PRICE_CATALOG.iter().find(|price| {
            price.provider == provider
                && normalized.contains(&price.model_pattern.to_ascii_lowercase())
        })
    });
    let input_tokens = usage.input_tokens.unwrap_or(0);
    let output_tokens = usage.output_tokens.unwrap_or(0);
    let cached_input_tokens = usage.cached_input_tokens.unwrap_or(0);
    let cache_write_input_tokens = usage.cache_write_input_tokens.unwrap_or(0);
    let reasoning_tokens = usage.reasoning_tokens.unwrap_or(0);
    let confidence = usage.confidence.unwrap_or(UsageConfidence::Estimated);
    let amount_usd = price.map(|price| {
        let categorized_input = cached_input_tokens + cache_write_input_tokens;
        let uncached_input = if provider == "claude" {
            input_tokens.max(0) as f64
        } else {
            (input_tokens - categorized_input).max(0) as f64
        };
        let cached_rate = price
            .cached_input_per_million_usd
            .unwrap_or(price.input_per_million_usd);
        let cache_write_rate = price
            .cache_write_per_million_usd
            .unwrap_or(price.input_per_million_usd);
        (uncached_input * price.input_per_million_usd
            + cached_input_tokens as f64 * cached_rate
            + cache_write_input_tokens as f64 * cache_write_rate
            + output_tokens as f64 * price.output_per_million_usd)
            / 1_000_000.0
    });

    CostEstimate {
        model,
        catalog_model: price.map(|price| price.catalog_model.to_string()),
        input_tokens,
        output_tokens,
        cached_input_tokens,
        cache_write_input_tokens,
        reasoning_tokens,
        amount_usd,
        confidence,
        pricing_date: price.map(|price| price.effective_date.to_string()),
        source_url: price.map(|price| price.source_url.to_string()),
        note: if price.is_some() {
            "Observed cache reads and cache writes are included when recorded. Tool-call fees, cache storage, long-context multipliers, regional uplifts, and subscription pricing are not included."
                .to_string()
        } else {
            "No matching dated API price was found for this recorded model.".to_string()
        },
    }
}

fn approximate_tokens(text: &str) -> i64 {
    if text.is_empty() {
        0
    } else {
        ((text.chars().count() as f64 / 4.0).ceil() as i64).max(1)
    }
}

fn tool_tokens(turn: &crate::model::Turn) -> i64 {
    turn.tool_calls
        .iter()
        .map(|tool| {
            tool.arguments_json
                .as_deref()
                .map(approximate_tokens)
                .unwrap_or(0)
                + tool
                    .result_text
                    .as_deref()
                    .map(approximate_tokens)
                    .unwrap_or(0)
        })
        .sum()
}
