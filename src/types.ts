export type Provider = "codex" | "claude" | "grok" | "antigravity";
export type ThemeId = "light" | "dark" | "graphite" | "sepia" | "petrol" | "plum" | "coral";
export type FontId = "archivo" | "manrope" | "source-serif";

export interface SourceCandidate {
  provider: Provider;
  kind: string;
  path: string;
  archived: boolean;
  modified_at_ms?: number;
  size_bytes: number;
}

export interface ProviderDiscovery {
  provider: Provider;
  installed: boolean;
  roots: string[];
  candidates: SourceCandidate[];
  warnings: string[];
}

export interface DiscoveryReport {
  providers: ProviderDiscovery[];
}

export interface ImportReport {
  discovered: number;
  imported: number;
  unchanged: number;
  failed: number;
  warnings: string[];
}

export interface ImportProgress {
  processed: number;
  total: number;
  provider: Provider;
  source_name: string;
  status: "reading" | "unchanged" | "complete" | "error";
}

export interface SessionListItem {
  id: string;
  provider: Provider;
  title: string;
  project_path?: string;
  source_path?: string;
  created_at?: string;
  updated_at?: string;
  model?: string;
  archived: boolean;
  turn_count: number;
  tool_call_count: number;
  total_tokens?: number;
  source_turn_count?: number;
}

export interface TokenUsage {
  input_tokens?: number;
  output_tokens?: number;
  cached_input_tokens?: number;
  cache_write_input_tokens?: number;
  reasoning_tokens?: number;
  total_tokens?: number;
  confidence?: "observed" | "reconstructed" | "estimated";
  source?: string;
}

export interface ToolCall {
  external_id?: string;
  name: string;
  arguments_json?: string;
  result_text?: string;
  status?: string;
  duration_ms?: number;
}

export interface Turn {
  external_id?: string;
  ordinal: number;
  prompt_ordinal?: number;
  role: "user" | "assistant" | "system" | "tool" | "reasoning" | "unknown";
  created_at?: string;
  text: string;
  event_type?: string;
  model?: string;
  parent_external_id?: string;
  usage?: TokenUsage;
  tool_calls: ToolCall[];
  metadata_json?: string;
}

export interface StoredSession {
  session: SessionListItem;
  summary?: string;
  turns: Turn[];
}

export interface CostEstimate {
  model?: string;
  catalog_model?: string;
  input_tokens: number;
  output_tokens: number;
  cached_input_tokens: number;
  cache_write_input_tokens: number;
  reasoning_tokens: number;
  amount_usd?: number;
  confidence: "observed" | "reconstructed" | "estimated";
  pricing_date?: string;
  source_url?: string;
  note: string;
}

export interface SessionDetail {
  data: StoredSession;
  cost: CostEstimate;
  total_turns: number;
  has_more: boolean;
}

export interface TurnPage {
  turns: Turn[];
  offset: number;
  total: number;
  has_more: boolean;
}

export interface ProviderUsage {
  provider: Provider;
  sessions: number;
  prompts: number;
  assistant_turns: number;
  tool_calls: number;
  total_tokens: number;
  active_days: number;
  average_prompts_per_session: number;
}

export interface DailyUsage {
  date: string;
  provider: Provider;
  sessions: number;
  prompts: number;
  assistant_turns: number;
  tool_calls: number;
  total_tokens: number;
}

export interface ToolUsage {
  provider: Provider;
  name: string;
  calls: number;
}

export interface ModelUsage {
  provider: Provider;
  model: string;
  sessions: number;
  prompts: number;
  assistant_turns: number;
  tool_calls: number;
  total_tokens: number;
}

export interface UsageAnalytics {
  providers: ProviderUsage[];
  days: DailyUsage[];
  top_tools: ToolUsage[];
  models: ModelUsage[];
  busiest_day?: DailyUsage;
  total_sessions: number;
  total_prompts: number;
  total_assistant_turns: number;
  total_tool_calls: number;
  total_tokens: number;
}

export interface FileReference {
  path: string;
  exists: boolean;
  is_image: boolean;
  origins: Array<"user" | "assistant" | "tool" | "system" | "unknown">;
}

export interface FileCollectionReport {
  destination: string;
  report_path: string;
  selected_references: number;
  copied_files: number;
  copied_bytes: number;
  missing: number;
  skipped: number;
  duplicates: number;
}

export interface UsageCostRow {
  session_id: string;
  provider: Provider;
  cost: CostEstimate;
}

export interface PriceSetting {
  id: string;
  provider: Provider;
  model_pattern: string;
  catalog_model: string;
  input_per_million_usd?: number;
  cached_input_per_million_usd?: number;
  cache_write_per_million_usd?: number;
  output_per_million_usd?: number;
  effective_date?: string;
  source_url?: string;
  built_in: boolean;
}

export interface AppInfo {
  database_path: string;
  portable: boolean;
  version: string;
}
