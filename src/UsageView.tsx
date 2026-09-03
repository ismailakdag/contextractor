import { useEffect, useMemo, useState } from "react";
import { Activity, ArrowLeft, CalendarDays, Coins, MessageSquareReply, MousePointerClick, Wrench } from "lucide-react";
import { getUsageAnalytics, getUsageCosts } from "./bridge";
import { applyPriceOverride } from "./prices";
import type { PriceSetting, Provider, SessionListItem, UsageAnalytics, UsageCostRow } from "./types";

const labels: Record<Provider, string> = { codex: "Codex", claude: "Claude", grok: "Grok", antigravity: "Antigravity" };
const providerOrder: Provider[] = ["codex", "claude", "grok", "antigravity"];
type Range = "all" | "30" | "90";
interface DayRow { date: string; prompts: number; responses: number; tools: number; sessions: number; tokens: number }

interface UsageViewProps {
  provider?: Provider;
  prices: PriceSetting[];
  sessions: SessionListItem[];
  onProvider: (provider?: Provider) => void;
  onBack: () => void;
  onOpenSession: (id: string, provider: Provider) => void;
  onSearchTool: (name: string) => void;
}

export function UsageView({ provider, prices, sessions, onProvider, onBack, onOpenSession, onSearchTool }: UsageViewProps) {
  const [data, setData] = useState<UsageAnalytics | null>(null);
  const [costRows, setCostRows] = useState<UsageCostRow[]>([]);
  const [range, setRange] = useState<Range>("all");
  const [selectedDay, setSelectedDay] = useState<string>();
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setData(null);
    setError(null);
    setSelectedDay(undefined);
    void Promise.all([getUsageAnalytics(provider), getUsageCosts(provider)])
      .then(([analytics, costs]) => {
        if (!active) return;
        setData(analytics);
        setCostRows(costs);
      })
      .catch((reason) => active && setError(String(reason)));
    return () => { active = false; };
  }, [provider]);

  const costs = useMemo(() => {
    const byProvider = new Map<Provider, { amount: number; priced: number; missing: number }>();
    const bySession = new Map<string, number | null>();
    let amount = 0;
    let priced = 0;
    let missing = 0;
    for (const row of costRows) {
      const adjusted = applyPriceOverride(row.cost, row.provider, prices);
      bySession.set(row.session_id, adjusted.amount_usd ?? null);
      const bucket = byProvider.get(row.provider) ?? { amount: 0, priced: 0, missing: 0 };
      if (adjusted.amount_usd == null) { missing += 1; bucket.missing += 1; }
      else { amount += adjusted.amount_usd; priced += 1; bucket.amount += adjusted.amount_usd; bucket.priced += 1; }
      byProvider.set(row.provider, bucket);
    }
    return { amount, priced, missing, byProvider, bySession };
  }, [costRows, prices]);

  const days = useMemo(() => {
    const aggregated = new Map<string, DayRow>();
    data?.days.forEach((day) => {
      const row = aggregated.get(day.date) ?? { date: day.date, prompts: 0, responses: 0, tools: 0, sessions: 0, tokens: 0 };
      row.prompts += day.prompts; row.responses += day.assistant_turns; row.tools += day.tool_calls; row.sessions += day.sessions; row.tokens += day.total_tokens;
      aggregated.set(day.date, row);
    });
    const all = [...aggregated.values()].sort((a, b) => a.date.localeCompare(b.date));
    if (range === "all" || !all.length) return all;
    const end = new Date(`${all.at(-1)!.date}T12:00:00`);
    const start = new Date(end);
    start.setDate(start.getDate() - Number(range) + 1);
    return all.filter((day) => new Date(`${day.date}T12:00:00`) >= start);
  }, [data, range]);

  const busiestDays = useMemo(() => [...days].sort((a, b) => activity(b) - activity(a)).slice(0, 10), [days]);
  const selected = days.find((day) => day.date === selectedDay) ?? busiestDays[0];
  const maximum = Math.max(1, ...days.map(activity));
  const span = days.length ? `${formatDate(days[0].date)} — ${formatDate(days.at(-1)!.date)}` : "Kayıtlı tarih yok";
  const visibleSessions = useMemo(() => sessions
    .filter((session) => !provider || session.provider === provider)
    .sort((a, b) => (b.updated_at || b.created_at || "").localeCompare(a.updated_at || a.created_at || ""))
    .slice(0, 40), [provider, sessions]);

  if (error) return <section className="insight-surface usage-empty"><Activity /><h2>Kullanım verisi okunamadı</h2><p>{error}</p></section>;
  if (!data) return <section className="insight-surface usage-loading"><div className="loading-rule" /><div className="loading-block wide" /><div className="loading-block" /></section>;

  return (
    <section className="insight-surface usage-view">
      <button className="usage-back" onClick={onBack}><ArrowLeft size={14} /> Arşive dön</button>
      <header className="surface-heading usage-heading">
        <div><h1>Kullanım</h1><p>{provider ? `${labels[provider]} için yerel aktivite` : "Tüm sağlayıcılardaki yerel çalışma geçmişi"}</p></div>
        <label className="range-control"><span>Dönem</span><select value={range} onChange={(event) => { setRange(event.target.value as Range); setSelectedDay(undefined); }}><option value="all">Tüm zamanlar</option><option value="90">Son 90 gün</option><option value="30">Son 30 gün</option></select></label>
      </header>

      <nav className="usage-filter-strip" aria-label="Kullanım sağlayıcısı">
        <button className={!provider ? "active" : ""} onClick={() => onProvider(undefined)}>Tümü</button>
        {providerOrder.map((item) => <button key={item} className={provider === item ? "active" : ""} onClick={() => onProvider(item)}>{labels[item]}</button>)}
      </nav>

      <div className="usage-ledger-top six">
        <div><MousePointerClick /><span>Prompt</span><strong>{format(data.total_prompts)}</strong></div>
        <div><MessageSquareReply /><span>Cevap</span><strong>{format(data.total_assistant_turns)}</strong></div>
        <div><Wrench /><span>Araç çağrısı</span><strong>{format(data.total_tool_calls)}</strong></div>
        <div><Activity /><span>Oturum</span><strong>{format(data.total_sessions)}</strong></div>
        <div><Coins /><span>API karşılığı</span><strong>{costs.priced ? currency(costs.amount) : "—"}</strong><small>{costs.missing ? `${costs.missing} eşleşme eksik` : `${costs.priced} oturum fiyatlandı`}</small></div>
        <div><CalendarDays /><span>En yoğun gün</span><strong>{selected ? formatDate(selected.date) : "—"}</strong><small>{selected ? `${activity(selected)} olay` : span}</small></div>
      </div>

      <div className="usage-section">
        <div className="section-heading"><h2>Sağlayıcı dökümü</h2><span>Satıra basarak filtrele · {span}</span></div>
        <div className="usage-table-scroll"><div className="provider-usage-table detailed">
          <div className="usage-table-head"><span>Sağlayıcı</span><span>Oturum</span><span>Prompt</span><span>Cevap</span><span>Araç</span><span>Aktif gün</span><span>Ort.</span><span>Token</span><span>API</span></div>
          {data.providers.map((row) => { const price = costs.byProvider.get(row.provider); return (
            <button className="usage-table-row" key={row.provider} onClick={() => onProvider(row.provider)}>
              <span><i className={`usage-dot ${row.provider}`} />{labels[row.provider]}</span><span>{format(row.sessions)}</span><span>{format(row.prompts)}</span><span>{format(row.assistant_turns)}</span><span>{format(row.tool_calls)}</span><span>{format(row.active_days)}</span><span>{row.average_prompts_per_session.toFixed(1)}</span><span>{format(row.total_tokens)}</span><span title={price?.missing ? `${price.missing} modelin fiyatı eksik` : undefined}>{price?.priced ? currency(price.amount) : "—"}</span>
            </button>
          ); })}
        </div></div>
      </div>

      <div className="usage-section rhythm-section">
        <div className="section-heading"><h2>Aktivite ritmi</h2><span>{days.length} aktif gün · bir güne basarak ayrıntıyı gör</span></div>
        <div className="rhythm-scroll"><div className="rhythm-chart" style={{ minWidth: `${Math.max(680, days.length * 22)}px` }} aria-label="Seçili dönemin kullanım grafiği">
          {days.map((day) => <button className={`rhythm-column ${selected?.date === day.date ? "active" : ""}`} key={day.date} onClick={() => setSelectedDay(day.date)} title={`${formatDate(day.date)} · ${day.prompts} prompt · ${day.responses} cevap · ${day.tools} araç`}><span style={{ height: `${Math.max(4, (activity(day) / maximum) * 100)}%` }} /><small>{day.date.slice(5)}</small></button>)}
        </div></div>
        {selected && <div className="selected-day-detail"><strong>{formatDate(selected.date)}</strong><span>{format(selected.sessions)} oturum</span><span>{format(selected.prompts)} prompt</span><span>{format(selected.responses)} cevap</span><span>{format(selected.tools)} araç</span><span>{format(selected.tokens)} token</span></div>}
      </div>

      <div className="usage-split">
        <div className="usage-section"><div className="section-heading"><h2>En çok kullanılan araçlar</h2><span>Arşivde bulmak için tıkla</span></div><div className="ranking-list">
          {data.top_tools.slice(0, 15).map((tool, index) => <button key={`${tool.provider}-${tool.name}`} onClick={() => onSearchTool(tool.name)}><b>{String(index + 1).padStart(2, "0")}</b><span><strong>{tool.name}</strong><small>{labels[tool.provider]}</small></span><em>{format(tool.calls)}</em></button>)}
          {!data.top_tools.length && <p className="usage-no-data">Araç çağrısı bulunamadı.</p>}
        </div></div>
        <div className="usage-section"><div className="section-heading"><h2>En yoğun günler</h2><span>Prompt + cevap + araç</span></div><div className="ranking-list day-ranking">
          {busiestDays.map((day, index) => <button key={day.date} className={selected?.date === day.date ? "active" : ""} onClick={() => setSelectedDay(day.date)}><b>{String(index + 1).padStart(2, "0")}</b><span><strong>{formatDate(day.date)}</strong><small>{day.sessions} oturum · {day.prompts} prompt</small></span><em>{format(activity(day))}</em></button>)}
        </div></div>
      </div>

      <div className="usage-section">
        <div className="section-heading"><h2>Model dökümü</h2><span>Yerel kayıtlarda belirtilen modeller</span></div>
        <div className="usage-table-scroll"><div className="model-usage-table">
          <div className="model-table-row head"><span>Model</span><span>Sağlayıcı</span><span>Oturum</span><span>Prompt</span><span>Cevap</span><span>Araç</span><span>Token</span></div>
          {data.models.map((row) => <div className="model-table-row" key={`${row.provider}-${row.model}`}><strong title={row.model}>{row.model}</strong><span>{labels[row.provider]}</span><span>{format(row.sessions)}</span><span>{format(row.prompts)}</span><span>{format(row.assistant_turns)}</span><span>{format(row.tool_calls)}</span><span>{format(row.total_tokens)}</span></div>)}
        </div></div>
      </div>

      <div className="usage-section">
        <div className="section-heading"><h2>Oturum günlüğü</h2><span>En son {visibleSessions.length} kayıt · açmak için tıkla</span></div>
        <div className="session-usage-list">
          {visibleSessions.map((session) => <button key={session.id} onClick={() => onOpenSession(session.id, session.provider)}><span className="session-usage-provider"><i className={`usage-dot ${session.provider}`} />{labels[session.provider]}</span><strong title={session.title}>{session.title}</strong><span>{formatDateTime(session.updated_at || session.created_at)}</span><span>{session.model || "Model yok"}</span><span>{format(session.turn_count)} tur · {format(session.tool_call_count)} araç</span><em>{costs.bySession.get(session.id) != null ? currency(costs.bySession.get(session.id)!) : "Fiyat yok"}</em></button>)}
        </div>
      </div>
    </section>
  );
}

function activity(day: Pick<DayRow, "prompts" | "responses" | "tools">) { return day.prompts + day.responses + day.tools; }
function format(value: number) { return new Intl.NumberFormat("tr-TR", { notation: value > 9999 ? "compact" : "standard", maximumFractionDigits: 1 }).format(value); }
function currency(value: number) { return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", minimumFractionDigits: value < 1 ? 2 : 0, maximumFractionDigits: value < 1 ? 4 : 2 }).format(value); }
function formatDate(value: string) { const date = new Date(`${value}T12:00:00`); return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat("tr-TR", { day: "numeric", month: "short", year: "numeric", timeZone: "Europe/Istanbul" }).format(date); }
function formatDateTime(value?: string) { if (!value) return "Tarih yok"; const date = new Date(value); return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat("tr-TR", { day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit", timeZone: "Europe/Istanbul" }).format(date); }
