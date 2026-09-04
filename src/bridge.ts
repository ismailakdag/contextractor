import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppInfo,
  DiscoveryReport,
  FileReference,
  FileCollectionReport,
  ImportProgress,
  ImportReport,
  SessionDetail,
  SessionListItem,
  TurnPage,
  ToolCall,
  UsageAnalytics,
  UsageCostRow,
} from "./types";
import { demoDetail, demoDiscovery, demoSessions } from "./demo";

export const isDesktop = "__TAURI_INTERNALS__" in window;

export async function getAppInfo(): Promise<AppInfo> {
  if (!isDesktop) {
    return { database_path: "Browser preview", portable: false, version: "0.1.0-preview" };
  }
  return invoke("app_info");
}

export async function discoverSources(): Promise<DiscoveryReport> {
  return isDesktop ? invoke("discover_sources") : demoDiscovery;
}

export async function scanSources(): Promise<ImportReport> {
  if (!isDesktop) {
    await new Promise((resolve) => setTimeout(resolve, 900));
    return { discovered: 83, imported: 0, unchanged: 83, failed: 0, warnings: [] };
  }
  return invoke("scan_sources");
}

export async function listSessions(
  provider?: string,
  search?: string,
): Promise<SessionListItem[]> {
  if (!isDesktop) {
    return demoSessions.filter((session) => {
      const providerMatch = !provider || session.provider === provider;
      const searchMatch = !search || session.title.toLowerCase().includes(search.toLowerCase());
      return providerMatch && searchMatch;
    });
  }
  return invoke("list_sessions", { provider: provider || null, search: search || null, limit: 500 });
}

export async function getSession(id: string): Promise<SessionDetail | null> {
  return isDesktop ? invoke("get_session", { id }) : demoDetail[id] ?? null;
}

export async function getSessionTurns(
  id: string,
  mode: string,
  offset = 0,
  limit = 80,
  search?: string,
): Promise<TurnPage> {
  if (!isDesktop) {
    const source = demoDetail[id]?.data.turns ?? [];
    const filtered = source.filter((turn) => {
      if (mode === "conversation") return turn.role === "user" || turn.role === "assistant";
      if (mode === "prompts") return turn.role === "user";
      if (mode === "system") return turn.role === "system";
      if (mode === "responses") return turn.role === "assistant";
      if (mode === "tools") return turn.role === "tool" || turn.tool_calls.length > 0;
      return turn.role !== "reasoning";
    });
    const turns = filtered.slice(offset, offset + limit);
    return { turns, offset, total: filtered.length, has_more: offset + turns.length < filtered.length };
  }
  return invoke("get_session_turns", { id, mode, offset, limit, search: search || null });
}

export async function getToolCall(id: string, turnOrdinal: number, toolOrdinal: number): Promise<ToolCall | null> {
  if (!isDesktop) return demoDetail[id]?.data.turns.find((turn) => turn.ordinal === turnOrdinal)?.tool_calls[toolOrdinal] ?? null;
  return invoke("get_tool_call", { id, turnOrdinal, toolOrdinal });
}

export async function getSessionFiles(id: string): Promise<FileReference[]> {
  if (!isDesktop) return [];
  return invoke("get_session_files", { id });
}

export async function collectSessionFiles(id: string, destination: string | undefined, includeWorkspace: boolean, originFilter: "user" | "assistant" | "all"): Promise<FileCollectionReport> {
  if (!isDesktop) throw new Error("Dosya paketi yalnızca masaüstü uygulamasında oluşturulabilir.");
  return invoke("collect_session_files", { id, destination: destination || null, includeWorkspace, originFilter });
}

export async function revealPath(path: string): Promise<string> {
  if (!isDesktop) throw new Error("Dosya açma yalnızca masaüstü uygulamasında kullanılabilir.");
  return invoke("reveal_path", { path });
}

export async function getUsageAnalytics(provider?: string): Promise<UsageAnalytics> {
  if (!isDesktop) {
    const rows = demoSessions.filter((session) => !provider || session.provider === provider);
    const providers = Object.values(
      rows.reduce<Record<string, UsageAnalytics["providers"][number]>>((acc, session) => {
        const row = acc[session.provider] ?? {
          provider: session.provider,
          sessions: 0,
          prompts: 0,
          assistant_turns: 0,
          tool_calls: 0,
          total_tokens: 0,
          active_days: 1,
          average_prompts_per_session: 0,
        };
        row.sessions += 1;
        row.prompts += Math.ceil(session.turn_count / 2);
        row.assistant_turns += Math.floor(session.turn_count / 2);
        row.tool_calls += session.tool_call_count;
        row.total_tokens += session.total_tokens ?? 0;
        row.average_prompts_per_session = row.prompts / row.sessions;
        acc[session.provider] = row;
        return acc;
      }, {}),
    );
    const days: UsageAnalytics["days"] = providers.map((row, index) => ({
      date: `2026-09-0${Math.max(1, 2 - index)}`,
      provider: row.provider,
      sessions: row.sessions,
      prompts: row.prompts,
      assistant_turns: row.assistant_turns,
      tool_calls: row.tool_calls,
      total_tokens: row.total_tokens,
    }));
    const top_tools: UsageAnalytics["top_tools"] = providers.map((row, index) => ({
      provider: row.provider,
      name: ["exec_command", "read_file", "browser_open", "search"][index] ?? "tool_call",
      calls: Math.max(1, row.tool_calls),
    }));
    const models: UsageAnalytics["models"] = providers.map((row) => ({
      provider: row.provider,
      model: row.provider === "codex" ? "gpt-5.6" : row.provider === "claude" ? "claude-opus" : "Bilinmiyor",
      sessions: row.sessions,
      prompts: row.prompts,
      assistant_turns: row.assistant_turns,
      tool_calls: row.tool_calls,
      total_tokens: row.total_tokens,
    }));
    return {
      providers,
      days,
      top_tools,
      models,
      busiest_day: days[0],
      total_sessions: providers.reduce((sum, row) => sum + row.sessions, 0),
      total_prompts: providers.reduce((sum, row) => sum + row.prompts, 0),
      total_assistant_turns: providers.reduce((sum, row) => sum + row.assistant_turns, 0),
      total_tool_calls: providers.reduce((sum, row) => sum + row.tool_calls, 0),
      total_tokens: providers.reduce((sum, row) => sum + row.total_tokens, 0),
    };
  }
  return invoke("usage_analytics", { provider: provider || null });
}

export async function getUsageCosts(provider?: string): Promise<UsageCostRow[]> {
  if (!isDesktop) return [];
  return invoke("usage_costs", { provider: provider || null });
}

export async function exportSession(
  sessionId: string,
  format: string,
  path: string,
  search?: string,
): Promise<void> {
  if (!isDesktop) {
    throw new Error("Export is available in the desktop build.");
  }
  return invoke("export_session_to_file", {
    sessionId,
    format,
    path,
    search: search || null,
    options: {
      include_tool_calls: true,
      include_tool_results: true,
      include_reasoning: false,
    },
  });
}

export async function exportArchive(
  provider: string | undefined,
  search: string | undefined,
  format: string,
  path: string,
): Promise<number> {
  if (!isDesktop) throw new Error("Toplu export yalnızca masaüstü uygulamasında kullanılabilir.");
  return invoke("export_archive_to_file", {
    provider: provider || null,
    search: search || null,
    format,
    path,
    options: { include_tool_calls: true, include_tool_results: true, include_reasoning: false },
  });
}

export async function onScanProgress(
  callback: (progress: ImportProgress) => void,
): Promise<UnlistenFn> {
  if (!isDesktop) {
    return () => undefined;
  }
  return listen<ImportProgress>("scan-progress", (event) => callback(event.payload));
}
