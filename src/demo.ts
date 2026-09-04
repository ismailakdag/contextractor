import type { DiscoveryReport, SessionDetail, SessionListItem, Turn } from "./types";

export const demoDiscovery: DiscoveryReport = {
  providers: [
    { provider: "codex", installed: true, roots: ["~/.codex/sessions"], candidates: Array(46).fill(null), warnings: [] },
    { provider: "claude", installed: true, roots: ["~/.claude/projects"], candidates: Array(14).fill(null), warnings: [] },
    { provider: "grok", installed: true, roots: ["~/.grok/sessions"], candidates: Array(13).fill(null), warnings: [] },
    { provider: "antigravity", installed: true, roots: ["~/.gemini/antigravity/brain", "~/.gemini/antigravity-cli/brain"], candidates: Array(15).fill(null), warnings: [] },
  ] as DiscoveryReport["providers"],
};

export const demoSessions: SessionListItem[] = [
  {
    id: "session-1",
    provider: "codex",
    title: "Örnek · Proje planını hazırla",
    created_at: "2026-09-02T15:30:22Z",
    updated_at: "2026-09-02T16:47:04Z",
    model: "gpt-5.6-sol",
    archived: false,
    turn_count: 28,
    tool_call_count: 17,
    total_tokens: 68420,
  },
  {
    id: "session-2",
    provider: "claude",
    title: "Örnek · Toplantı notlarını özetle",
    created_at: "2026-08-29T09:11:00Z",
    updated_at: "2026-08-29T10:02:18Z",
    model: "claude-opus-4.7",
    archived: false,
    turn_count: 19,
    tool_call_count: 31,
    total_tokens: 44802,
  },
  {
    id: "session-3",
    provider: "antigravity",
    title: "Örnek · Veri aktarımını gözden geçir",
    created_at: "2026-08-24T12:40:00Z",
    updated_at: "2026-08-24T15:16:10Z",
    model: "gemini-3.7-flash",
    archived: false,
    turn_count: 42,
    tool_call_count: 24,
  },
  {
    id: "session-4",
    provider: "grok",
    title: "Örnek · Yayın kontrol listesi oluştur",
    created_at: "2026-08-21T18:04:00Z",
    updated_at: "2026-08-21T18:51:38Z",
    model: "grok-4.6",
    archived: false,
    turn_count: 15,
    tool_call_count: 8,
    total_tokens: 22810,
  },
];

const turns: Turn[] = [
  {
    ordinal: 0,
    role: "user",
    created_at: "2026-09-02T15:30:22Z",
    text: "Bir ürün güncellemesi için kısa ve uygulanabilir bir yayın kontrol listesi hazırla.",
    event_type: "message",
    tool_calls: [],
  },
  {
    ordinal: 1,
    role: "assistant",
    created_at: "2026-09-02T15:31:08Z",
    text: "Kontrol listesini hazırlık, doğrulama ve yayın sonrası takip olmak üzere üç bölüme ayıracağım.",
    event_type: "message",
    model: "gpt-5.6-sol",
    usage: {
      input_tokens: 12940,
      output_tokens: 812,
      cached_input_tokens: 8400,
      cache_write_input_tokens: 0,
      reasoning_tokens: 316,
      total_tokens: 13752,
      confidence: "observed",
      source: "codex:event_msg/token_count",
    },
    tool_calls: [],
  },
  {
    ordinal: 2,
    role: "tool",
    created_at: "2026-09-02T15:32:14Z",
    text: "",
    event_type: "custom_tool_call",
    tool_calls: [
      {
        external_id: "call-01",
        name: "check_project_status",
        arguments_json: '{"scope":"current_project"}',
        result_text: "Project status checked successfully",
        status: "completed",
        duration_ms: 742,
      },
    ],
  },
  {
    ordinal: 3,
    role: "assistant",
    created_at: "2026-09-02T15:33:40Z",
    text: "Kontrol listesi hazır: sürüm notlarını doğrula, testleri çalıştır, paketi üret ve yayın sonrası temel kontrolleri tamamla.",
    event_type: "message",
    model: "gpt-5.6-sol",
    tool_calls: [],
  },
];

export const demoDetail: Record<string, SessionDetail> = Object.fromEntries(
  demoSessions.map((session) => [
    session.id,
    {
      data: {
        session,
        summary:
          "Örnek bir ürün güncellemesi için hazırlık, doğrulama ve takip adımları çıkarıldı.",
        turns,
      },
      cost: {
        model: session.model,
        catalog_model: session.model,
        input_tokens: session.total_tokens ? Math.round(session.total_tokens * 0.78) : 18400,
        output_tokens: session.total_tokens ? Math.round(session.total_tokens * 0.22) : 5100,
        cached_input_tokens: session.total_tokens ? Math.round(session.total_tokens * 0.31) : 0,
        cache_write_input_tokens: 0,
        reasoning_tokens: 1820,
        amount_usd: session.provider === "codex" ? 0.62 : session.provider === "claude" ? 0.74 : 0.18,
        confidence: session.total_tokens ? "observed" : "estimated",
        pricing_date: "2026-09-02",
        note: "Tool-call fees and long-context multipliers are not included.",
      },
      total_turns: turns.length,
      has_more: false,
    },
  ]),
);
