import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { open as openDialog, save } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  Archive,
  ArrowDown,
  ArrowUp,
  BarChart3,
  Check,
  ChevronDown,
  Database,
  Download,
  FileText,
  Folder,
  FolderOutput,
  ImageOff,
  Palette,
  RefreshCw,
  Search,
  ShieldCheck,
  SlidersHorizontal,
  Wrench,
  X,
} from "lucide-react";
import {
  discoverSources,
  exportArchive,
  exportSession,
  getAppInfo,
  collectSessionFiles,
  getSession,
  getSessionFiles,
  getSessionTurns,
  getToolCall,
  isDesktop,
  listSessions,
  onScanProgress,
  revealPath,
  scanSources,
} from "./bridge";
import { applyPriceOverride, loadPrices, savePrices } from "./prices";
import { CopyAction } from "./CopyAction";
import { SettingsView } from "./SettingsView";
import { UsageView } from "./UsageView";
import { cleanDisplayText } from "./text";
import type {
  AppInfo,
  CostEstimate,
  DiscoveryReport,
  FileReference,
  FileCollectionReport,
  FontId,
  ImportProgress,
  PriceSetting,
  SessionDetail,
  SessionListItem,
  ThemeId,
  ToolCall,
  Turn,
} from "./types";

const providerMeta = {
  codex: { label: "Codex", mark: "CX" },
  claude: { label: "Claude", mark: "CL" },
  grok: { label: "Grok", mark: "GK" },
  antigravity: { label: "AGY", mark: "AG" },
} as const;

type ProviderFilter = keyof typeof providerMeta | "all";
type ViewMode = "conversation" | "all" | "prompts" | "responses" | "system" | "tools" | "summary";
type AppMode = "archive" | "usage" | "settings";
type SessionSort = "newest" | "oldest";
const PAGE_SIZE = 120;
const THEME_IDS: ThemeId[] = ["light", "sepia", "coral", "dark", "graphite", "petrol", "plum"];

const MarkdownBody = lazy(() => import("./MarkdownBody").then((module) => ({ default: module.MarkdownBody })));
const HighlightedJson = lazy(() => import("./MarkdownBody").then((module) => ({ default: module.HighlightedJson })));

export default function App() {
  const [discovery, setDiscovery] = useState<DiscoveryReport | null>(null);
  const [sessions, setSessions] = useState<SessionListItem[]>([]);
  const [allSessions, setAllSessions] = useState<SessionListItem[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [provider, setProvider] = useState<ProviderFilter>("all");
  const [search, setSearch] = useState("");
  const [viewMode, setViewMode] = useState<ViewMode>("conversation");
  const [loadedMode, setLoadedMode] = useState<ViewMode>("conversation");
  const [loadedSearch, setLoadedSearch] = useState("");
  const [appMode, setAppMode] = useState<AppMode>("archive");
  const [bulkExportOpen, setBulkExportOpen] = useState(false);
  const [bulkExportScope, setBulkExportScope] = useState<ProviderFilter>("all");
  const [bulkExportUsesSearch, setBulkExportUsesSearch] = useState(true);
  const [turnLoading, setTurnLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [turnOffset, setTurnOffset] = useState(0);
  const [jumpLoading, setJumpLoading] = useState(false);
  const [sessionSort, setSessionSort] = useState<SessionSort>("newest");
  const [prices, setPrices] = useState<PriceSetting[]>(loadPrices);
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [sessionFiles, setSessionFiles] = useState<FileReference[]>([]);
  const [filesLoading, setFilesLoading] = useState(false);
  const [theme, setTheme] = useState<ThemeId>(() => {
    const saved = localStorage.getItem("contextractor-theme");
    if (THEME_IDS.includes(saved as ThemeId)) return saved as ThemeId;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  });
  const [fontFamily, setFontFamily] = useState<FontId>(() => {
    const saved = localStorage.getItem("contextractor-font-family");
    return saved === "manrope" || saved === "source-serif" ? saved : "archivo";
  });
  const [fontScale, setFontScale] = useState(() => Number(localStorage.getItem("contextractor-font-scale")) || 1);
  const searchRef = useRef<HTMLInputElement>(null);
  const sessionRequest = useRef(0);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("contextractor-theme", theme);
  }, [theme]);

  useEffect(() => {
    document.documentElement.dataset.font = fontFamily;
    localStorage.setItem("contextractor-font-family", fontFamily);
  }, [fontFamily]);

  useEffect(() => localStorage.setItem("contextractor-font-scale", String(fontScale)), [fontScale]);

  useEffect(() => setBulkExportScope(provider), [provider]);

  const loadSessions = useCallback(async (nextProvider: ProviderFilter, query: string) => {
    const request = ++sessionRequest.current;
    const found = await listSessions(nextProvider === "all" ? undefined : nextProvider, query);
    if (request !== sessionRequest.current) return;
    setSessions(found);
    setSelectedId((current) =>
      current && found.some((session) => session.id === current) ? current : found[0]?.id ?? null,
    );
  }, []);

  useEffect(() => {
    let active = true;
    void Promise.all([discoverSources(), getAppInfo(), listSessions()]).then(
      ([foundDiscovery, info, foundSessions]) => {
        if (!active) return;
        setDiscovery(foundDiscovery);
        setAppInfo(info);
        setSessions(foundSessions);
        setAllSessions(foundSessions);
        setSelectedId(foundSessions[0]?.id ?? null);
      },
    );
    const shortcut = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", shortcut);
    let unsubscribe: (() => void) | undefined;
    void onScanProgress(setProgress).then((fn) => (unsubscribe = fn));
    return () => {
      active = false;
      unsubscribe?.();
      window.removeEventListener("keydown", shortcut);
    };
  }, []);

  useEffect(() => {
    if (!selectedId) {
      setDetail(null);
      return;
    }
    let active = true;
    setDetail(null);
    setSessionFiles([]);
    setFilesLoading(true);
    setViewMode("conversation");
    setLoadedMode("conversation");
    setLoadedSearch("");
    setTurnOffset(0);
    void getSession(selectedId)
      .then(async (session) => {
        if (!active) return;
        if (!session) {
          setDetail(null);
          return;
        }
        if (session.total_turns <= PAGE_SIZE) {
          setTurnOffset(0);
          setDetail(session);
          return;
        }

        // Conversation inspectors are most useful at the live edge. Fetch the
        // last bounded page on open; older history remains available through
        // the explicit "previous records" control and the top jump.
        const offset = Math.max(0, session.total_turns - PAGE_SIZE);
        const page = await getSessionTurns(selectedId, "conversation", offset, PAGE_SIZE, "");
        if (!active) return;
        setTurnOffset(page.offset);
        setDetail({
          ...session,
          data: { ...session.data, turns: page.turns },
          total_turns: page.total,
          has_more: page.has_more,
        });
      })
      .catch((error) => active && setNotice(String(error)));
    void getSessionFiles(selectedId)
      .then((files) => active && setSessionFiles(files))
      .catch(() => active && setSessionFiles([]))
      .finally(() => active && setFilesLoading(false));
    return () => {
      active = false;
    };
  }, [selectedId]);

  useEffect(() => {
    if (appMode !== "archive") return;
    const timer = window.setTimeout(() => void loadSessions(provider, search), search ? 360 : 40);
    return () => window.clearTimeout(timer);
  }, [provider, search, appMode, loadSessions]);

  useEffect(() => {
    if (!selectedId || !detail || viewMode === "summary" || (viewMode === loadedMode && search.trim() === loadedSearch)) return;
    let active = true;
    setTurnLoading(true);
    void getSessionTurns(selectedId, viewMode, 0, PAGE_SIZE, search.trim()).then((page) => {
      if (!active) return;
      setDetail((current) => current ? { ...current, data: { ...current.data, turns: page.turns }, total_turns: page.total, has_more: page.has_more } : current);
      setLoadedMode(viewMode);
      setLoadedSearch(search.trim());
      setTurnOffset(0);
      setTurnLoading(false);
    }).catch((error) => {
      if (active) { setNotice(String(error)); setTurnLoading(false); }
    });
    return () => { active = false; };
  }, [viewMode, loadedMode, loadedSearch, search, selectedId, detail?.data.session.id]);

  useEffect(() => {
    if (!isDesktop) return;
    const timer = window.setTimeout(() => void runScan(), 350);
    return () => window.clearTimeout(timer);
  }, []);

  const runScan = async () => {
    if (scanning) return;
    setScanning(true);
    setNotice(null);
    try {
      const report = await scanSources();
      const found = await listSessions();
      setAllSessions(found);
      await Promise.all([loadSessions(provider, search), discoverSources().then(setDiscovery)]);
      setNotice(
        report.failed
          ? `${report.imported} oturum güncellendi · ${report.failed} kaynak incelenmeli`
          : `${report.imported} güncellendi · ${report.unchanged} değişmedi`,
      );
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setScanning(false);
      setProgress(null);
    }
  };

  const loadMoreTurns = async () => {
    if (!selectedId || !detail || loadingMore || !detail.has_more) return;
    setLoadingMore(true);
    try {
      const page = await getSessionTurns(selectedId, loadedMode, turnOffset + detail.data.turns.length, PAGE_SIZE, loadedSearch);
      setDetail((current) => current ? { ...current, data: { ...current.data, turns: [...current.data.turns, ...page.turns] }, total_turns: page.total, has_more: page.has_more } : current);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setLoadingMore(false);
    }
  };

  const loadPreviousTurns = async () => {
    if (!selectedId || !detail || loadingMore || turnOffset <= 0) return;
    setLoadingMore(true);
    try {
      const nextOffset = Math.max(0, turnOffset - PAGE_SIZE);
      const page = await getSessionTurns(selectedId, loadedMode, nextOffset, turnOffset - nextOffset, loadedSearch);
      setTurnOffset(page.offset);
      setDetail((current) => current ? {
        ...current,
        data: { ...current.data, turns: [...page.turns, ...current.data.turns] },
        total_turns: page.total,
        has_more: turnOffset + current.data.turns.length < page.total,
      } : current);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setLoadingMore(false);
    }
  };

  const jumpTurns = async (edge: "start" | "end") => {
    if (!selectedId || !detail || jumpLoading || viewMode === "summary") return;
    setJumpLoading(true);
    try {
      const offset = edge === "end" ? Math.max(0, detail.total_turns - PAGE_SIZE) : 0;
      const page = await getSessionTurns(selectedId, loadedMode, offset, PAGE_SIZE, loadedSearch);
      setTurnOffset(page.offset);
      setDetail((current) => current ? { ...current, data: { ...current.data, turns: page.turns }, total_turns: page.total, has_more: page.has_more } : current);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setJumpLoading(false);
    }
  };

  const changePrices = (next: PriceSetting[]) => {
    setPrices(next);
    savePrices(next);
  };

  const sortedSessions = useMemo(() => [...sessions].sort((left, right) => {
    const leftTime = Date.parse(left.updated_at || left.created_at || "") || 0;
    const rightTime = Date.parse(right.updated_at || right.created_at || "") || 0;
    return sessionSort === "newest" ? rightTime - leftTime : leftTime - rightTime;
  }), [sessions, sessionSort]);
  const selectedIndex = sortedSessions.findIndex((session) => session.id === selectedId);
  const moveSelection = (offset: number) => {
    if (!sortedSessions.length) return;
    const next = Math.min(Math.max(selectedIndex + offset, 0), sortedSessions.length - 1);
    setSelectedId(sortedSessions[next].id);
  };

  const doBulkExport = async (format: string) => {
    setBulkExportOpen(false);
    const extension = ["markdown", "prompts", "system", "context", "responses", "tools", "summary"].includes(format) ? "md" : format;
    const exportSearch = bulkExportUsesSearch ? search.trim() : "";
    const scope = bulkExportScope === "all" ? "tum-arsiv" : bulkExportScope;
    try {
      const destination = await save({
        defaultPath: `contextractor-${scope}${exportSearch ? "-filtreli" : ""}.${extension}`,
        filters: [{ name: format.toUpperCase(), extensions: [extension] }],
      });
      if (!destination) return;
      const count = await exportArchive(bulkExportScope === "all" ? undefined : bulkExportScope, exportSearch || undefined, format, destination);
      setNotice(`${count} oturum dışa aktarıldı · ${destination}`);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <div className="app-frame" style={{ "--ui-scale": fontScale } as CSSProperties}>
      <header className="topbar">
        <div className="brand-lockup">
          <ArchiveMark />
          <div>
            <strong>Contextractor</strong>
            <span>Local conversation archive</span>
          </div>
        </div>

        <label className="search-field">
          <Search size={16} strokeWidth={1.8} aria-hidden="true" />
          <input
            ref={searchRef}
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="Prompt, cevap veya araç ara"
            aria-label="Oturumlarda ara"
            onFocus={() => setAppMode("archive")}
          />
          {search ? (
            <button className="bare-button" onClick={() => setSearch("")} aria-label="Aramayı temizle">
              <X size={15} />
            </button>
          ) : (
            <kbd>Ctrl K</kbd>
          )}
        </label>

        <div className="topbar-actions">
          {!isDesktop && <span className="preview-flag">Preview data</span>}
          <span className="privacy-status">
            <ShieldCheck size={15} /> Yerel · salt okunur
          </span>
          <div className="font-controls" aria-label="Yazı boyutu">
            <button onClick={() => setFontScale((value) => Math.max(.85, Number((value - .1).toFixed(2))))} disabled={fontScale <= .85} aria-label="Yazıyı küçült">A−</button>
            <button onClick={() => setFontScale(1)} title="Varsayılan yazı boyutu">{Math.round(fontScale * 100)}%</button>
            <button onClick={() => setFontScale((value) => Math.min(1.3, Number((value + .1).toFixed(2))))} disabled={fontScale >= 1.3} aria-label="Yazıyı büyüt">A+</button>
          </div>
          <button
            className={appMode === "settings" ? "theme-button active" : "theme-button"}
            onClick={() => setAppMode("settings")}
            aria-label="Görünüm ayarlarını aç"
            title="Tema ve yazı ayarları"
          >
            <Palette size={16} />
          </button>
          <button className="scan-button" onClick={() => void runScan()} disabled={scanning}>
            <RefreshCw size={16} className={scanning ? "spin" : ""} />
            {scanning ? "Taranıyor" : "Yeniden tara"}
          </button>
          <div className="bulk-export-wrap">
            <button className="bulk-export-button" onClick={() => setBulkExportOpen((open) => !open)} aria-expanded={bulkExportOpen}><Download size={15} /> Export · {bulkExportScope === "all" ? "Tümü" : providerMeta[bulkExportScope].label}</button>
            {bulkExportOpen && <div className="export-menu bulk-export-menu" role="menu">
              <div className="bulk-export-scope">
                <label><span>Kapsam</span><select value={bulkExportScope} onChange={(event) => setBulkExportScope(event.target.value as ProviderFilter)}>
                  <option value="all">Tüm sağlayıcılar</option>
                  {Object.entries(providerMeta).map(([id, meta]) => <option key={id} value={id}>{meta.label}</option>)}
                </select></label>
                <small>{bulkExportScope === "all" ? allSessions.length : allSessions.filter((session) => session.provider === bulkExportScope).length} içerikli oturum</small>
                {search.trim() && <label className="bulk-search-option"><input type="checkbox" checked={bulkExportUsesSearch} onChange={(event) => setBulkExportUsesSearch(event.target.checked)} /> Yalnız “{search.trim()}” eşleşmeleri</label>}
              </div>
              {[['markdown','Tam konuşmalar','MD'],['prompts','Yalnız promptlar','MD'],['responses','Yalnız cevaplar','MD'],['system','System promptları','MD'],['tools','Araç kayıtları','MD'],['json','Normalize arşiv','JSON'],['jsonl','Oturum akışı','JSONL']].map(([format,label,ext]) => <button key={format} role="menuitem" onClick={() => void doBulkExport(format)}>{label}<span>{ext}</span></button>)}
            </div>}
          </div>
        </div>
      </header>

      {scanning && (
        <div className="scan-track" role="status" aria-live="polite">
          <div
            className="scan-fill"
            style={{ width: `${progress?.total ? (progress.processed / progress.total) * 100 : 3}%` }}
          />
          <span>
            {progress
              ? `${providerMeta[progress.provider].label} · ${progress.processed}/${progress.total}`
              : "Kaynaklar hazırlanıyor"}
          </span>
        </div>
      )}

      <main className={`workspace ${appMode !== "archive" ? "insight-open" : ""}`}>
        <ProviderRail
          discovery={discovery}
          sessions={sessions}
          selected={provider}
          onSelect={(next) => { setProvider(next); setAppMode("archive"); }}
          appInfo={appInfo}
          appMode={appMode}
          onAppMode={setAppMode}
        />
        {appMode === "archive" ? <>
          <SessionCatalog sessions={sortedSessions} selectedId={selectedId} search={search} sort={sessionSort} onSort={setSessionSort} onSelect={setSelectedId} onMove={moveSelection} />
          <Inspector detail={detail} prices={prices} files={sessionFiles} filesLoading={filesLoading} showEmpty={discovery !== null && sessions.length === 0} search={search.trim()} viewMode={viewMode} turnOffset={turnOffset} turnLoading={turnLoading} loadingMore={loadingMore} jumpLoading={jumpLoading} onLoadMore={() => void loadMoreTurns()} onLoadPrevious={() => void loadPreviousTurns()} onJump={jumpTurns} onViewMode={setViewMode} onNotice={setNotice} />
        </> : appMode === "usage" ? (
          <UsageView
            provider={provider === "all" ? undefined : provider}
            prices={prices}
            sessions={allSessions}
            onProvider={(next) => setProvider(next ?? "all")}
            onBack={() => setAppMode("archive")}
            onOpenSession={(id, nextProvider) => {
              setProvider(nextProvider);
              setSearch("");
              setSelectedId(id);
              setAppMode("archive");
            }}
            onSearchTool={(name) => {
              setSearch(name);
              setAppMode("archive");
            }}
          />
        ) : (
          <SettingsView
            prices={prices}
            sessions={allSessions}
            onChange={changePrices}
            theme={theme}
            fontFamily={fontFamily}
            onThemeChange={setTheme}
            onFontChange={setFontFamily}
          />
        )}
      </main>

      {notice && (
        <button className="notice" onClick={() => setNotice(null)} aria-label="Bildirimi kapat">
          <Check size={15} />
          <span>{notice}</span>
          <X size={14} />
        </button>
      )}
    </div>
  );
}

function ArchiveMark() {
  return (
    <svg className="archive-mark" viewBox="0 0 34 34" aria-hidden="true">
      <path d="M4 7.5h26M7.5 4v26M26.5 4v26M4 26.5h26" />
      <path d="M12 12h10v10H12z" />
      <circle cx="17" cy="17" r="2.2" />
    </svg>
  );
}

function ProviderRail({
  discovery,
  sessions,
  selected,
  onSelect,
  appInfo,
  appMode,
  onAppMode,
}: {
  discovery: DiscoveryReport | null;
  sessions: SessionListItem[];
  selected: ProviderFilter;
  onSelect: (provider: ProviderFilter) => void;
  appInfo: AppInfo | null;
  appMode: AppMode;
  onAppMode: (mode: AppMode) => void;
}) {
  const providerRows = Object.entries(providerMeta) as [keyof typeof providerMeta, (typeof providerMeta)[keyof typeof providerMeta]][];
  const total = discovery?.providers.reduce((sum, item) => sum + item.candidates.length, 0) ?? sessions.length;
  return (
    <nav className="provider-rail" aria-label="Sağlayıcılar">
      <div className="rail-heading">
        <span>Kaynaklar</span>
        <b>{total}</b>
      </div>
      <button className={selected === "all" ? "provider-row active" : "provider-row"} onClick={() => onSelect("all")}>
        <span className="provider-glyph all"><Database size={15} /></span>
        <span className="provider-label">Tüm oturumlar</span>
        <span className="provider-count">{total}</span>
      </button>
      {providerRows.map(([id, meta]) => {
        const source = discovery?.providers.find((item) => item.provider === id);
        const count = source?.candidates.length ?? sessions.filter((session) => session.provider === id).length;
        return (
          <button
            key={id}
            className={selected === id ? "provider-row active" : "provider-row"}
            onClick={() => onSelect(id)}
          >
            <span className={`provider-glyph ${id}`}>{meta.mark}</span>
            <span className="provider-label">
              {meta.label}
              <small>{source?.installed ? "bulundu" : "bulunamadı"}</small>
            </span>
            <span className="provider-count">{count}</span>
          </button>
        );
      })}
      <div className="rail-divider" />
      <button className={appMode === "usage" ? "rail-mode active" : "rail-mode"} onClick={() => onAppMode("usage")}>
        <BarChart3 size={15} /><span>Kullanım</span>
      </button>
      <button className={appMode === "settings" ? "rail-mode active" : "rail-mode"} onClick={() => onAppMode("settings")}>
        <SlidersHorizontal size={15} /><span>Ayarlar</span>
      </button>
      <div className="rail-spacer" />
      <div className="archive-location" title={appInfo?.database_path}>
        <Database size={14} />
        <span>
          Archive database
          <small>{appInfo?.portable ? "Portable data" : "Local app data"}</small>
        </span>
      </div>
    </nav>
  );
}

function SessionCatalog({
  sessions,
  selectedId,
  search,
  sort,
  onSort,
  onSelect,
  onMove,
}: {
  sessions: SessionListItem[];
  selectedId: string | null;
  search: string;
  sort: SessionSort;
  onSort: (sort: SessionSort) => void;
  onSelect: (id: string) => void;
  onMove: (offset: number) => void;
}) {
  return (
    <section className="session-catalog" aria-label="Oturum kataloğu">
      <header className="catalog-heading">
        <div>
          <h1>{search ? "Arama sonuçları" : "Oturumlar"}</h1>
          <span>{sessions.length} kayıt</span>
        </div>
        <label className="catalog-sort" title="Oturumları tarihe göre sırala">
          <span>Sırala</span>
          <select value={sort} onChange={(event) => onSort(event.target.value as SessionSort)} aria-label="Oturum sıralaması">
            <option value="newest">En yeni</option>
            <option value="oldest">En eski</option>
          </select>
        </label>
      </header>
      <div
        className="session-list"
        role="listbox"
        tabIndex={0}
        aria-label="Oturumlar"
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            onMove(1);
          }
          if (event.key === "ArrowUp") {
            event.preventDefault();
            onMove(-1);
          }
        }}
      >
        {sessions.length ? (
          sessions.map((session) => (
            <button
              key={session.id}
              role="option"
              aria-selected={selectedId === session.id}
              className={selectedId === session.id ? "session-row selected" : "session-row"}
              onClick={() => onSelect(session.id)}
            >
              <span className={`session-provider ${session.provider}`}>
                {providerMeta[session.provider].mark}
              </span>
              <span className="session-copy">
                <strong>{session.title}</strong>
                <span>{leafPath(session.project_path) || "Genel çalışma alanı"}</span>
                <small>
                  {formatDate(session.updated_at || session.created_at)}
                  <i /> {session.turn_count === 0 ? "yalnız metadata" : `${session.turn_count} tur`}
                  {session.tool_call_count > 0 && (
                    <>
                      <i /> {session.tool_call_count} araç
                    </>
                  )}
                </small>
              </span>
            </button>
          ))
        ) : (
          <div className="catalog-empty">
            <Search size={20} />
            <strong>Kayıt bulunamadı</strong>
            <span>Filtreyi veya arama ifadesini değiştir.</span>
          </div>
        )}
      </div>
    </section>
  );
}

function Inspector({
  detail,
  prices,
  files,
  filesLoading,
  showEmpty,
  search,
  viewMode,
  turnOffset,
  turnLoading,
  loadingMore,
  jumpLoading,
  onLoadMore,
  onLoadPrevious,
  onJump,
  onViewMode,
  onNotice,
}: {
  detail: SessionDetail | null;
  prices: PriceSetting[];
  files: FileReference[];
  filesLoading: boolean;
  showEmpty: boolean;
  search: string;
  viewMode: ViewMode;
  turnOffset: number;
  turnLoading: boolean;
  loadingMore: boolean;
  jumpLoading: boolean;
  onLoadMore: () => void;
  onLoadPrevious: () => void;
  onJump: (edge: "start" | "end") => Promise<void>;
  onViewMode: (mode: ViewMode) => void;
  onNotice: (message: string) => void;
}) {
  const [exportOpen, setExportOpen] = useState(false);
  const [evidenceOpen, setEvidenceOpen] = useState(false);
  useEffect(() => { setEvidenceOpen(false); setExportOpen(false); }, [detail?.data.session.id]);
  useEffect(() => {
    if (!exportOpen && !evidenceOpen) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setExportOpen(false);
        setEvidenceOpen(false);
      }
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [exportOpen, evidenceOpen]);
  if (!detail) {
    if (showEmpty) {
      return (
        <section className="inspector inspector-empty" aria-label="Oturum seçilmedi">
          <Archive size={22} aria-hidden="true" />
          <h2>Gösterilecek oturum yok</h2>
          <p>Başka bir sağlayıcı seçin veya arama ifadesini değiştirin.</p>
        </section>
      );
    }
    return (
      <section className="inspector loading-inspector" aria-label="Oturum yükleniyor">
        <div className="loading-rule" />
        <div className="loading-block wide" />
        <div className="loading-block" />
      </section>
    );
  }
  const { data } = detail;
  const cost = applyPriceOverride(detail.cost, data.session.provider, prices);
  const doExport = async (format: string) => {
    setExportOpen(false);
    const extension = ["markdown", "prompts", "system", "context", "responses", "tools", "summary"].includes(format) ? "md" : format;
    try {
      const destination = await save({
        defaultPath: `${safeFileName(data.session.title)}.${extension}`,
        filters: [{ name: format.toUpperCase(), extensions: [extension] }],
      });
      if (!destination) return;
      await exportSession(data.session.id, format, destination, search || undefined);
      onNotice(`${search ? "Filtrelenmiş kayıt" : "Oturum"} dışa aktarıldı · ${destination}`);
    } catch (error) {
      onNotice(error instanceof Error ? error.message : String(error));
    }
  };
  const showPath = async (path: string) => {
    try {
      const opened = await revealPath(path);
      onNotice(opened === path ? `Klasörde gösterildi · ${path}` : `En yakın konum açıldı · ${opened}`);
    } catch (error) {
      onNotice(String(error));
    }
  };

  return (
    <section className="inspector" aria-label="Seçili oturum">
      <header className="inspector-header">
        <div className="title-line">
          <span className={`record-stamp ${data.session.provider}`}>
            {providerMeta[data.session.provider].label}
          </span>
          {data.session.archived && <span className="archive-stamp">Arşiv</span>}
        </div>
        <div className="inspector-title-row">
          <div>
            <h2>{data.session.title}</h2>
            <div className="session-context">
              {data.session.project_path && (
                <button className="session-path-button" onClick={() => void showPath(data.session.project_path!)} title={`${data.session.project_path} · klasörde göster`}>
                  <Folder size={14} /> {data.session.project_path}
                </button>
              )}
              {data.session.model && <span>{data.session.model}</span>}
              <span>{formatDate(data.session.updated_at || data.session.created_at, true)}</span>
            </div>
          </div>
          <div className="export-wrap">
            <button className="export-button" onClick={() => setExportOpen((open) => !open)} aria-expanded={exportOpen}>
              <Download size={16} /> Dışa aktar <ChevronDown size={14} />
            </button>
            {exportOpen && (
              <div className="export-menu" role="menu">
                <button role="menuitem" onClick={() => void doExport("markdown")}>
                  Tam konuşma <span>MD</span>
                </button>
                <button role="menuitem" onClick={() => void doExport("prompts")}>
                  Yalnız promptlar <span>MD</span>
                </button>
                <button role="menuitem" onClick={() => void doExport("system")}>
                  System promptları <span>MD</span>
                </button>
                <button role="menuitem" onClick={() => void doExport("context")}>
                  System + kullanıcı <span>MD</span>
                </button>
                <button role="menuitem" onClick={() => void doExport("responses")}>
                  Yalnız cevaplar <span>MD</span>
                </button>
                <button role="menuitem" onClick={() => void doExport("tools")}>
                  Araç kayıtları <span>MD</span>
                </button>
                <button role="menuitem" onClick={() => void doExport("summary")}>
                  Oturum özeti <span>MD</span>
                </button>
                <button role="menuitem" onClick={() => void doExport("json")}>
                  Normalize veri <span>JSON</span>
                </button>
                <button role="menuitem" onClick={() => void doExport("jsonl")}>
                  Turn akışı <span>JSONL</span>
                </button>
              </div>
            )}
          </div>
          <button className="evidence-button" onClick={() => setEvidenceOpen(true)}>
            <FileText size={14} /><span>Kayıt dökümü</span>
          </button>
        </div>
        <div className="view-tabs" role="tablist" aria-label="Konuşma görünümü">
          {(
            [
              ["conversation", "Konuşma"],
              ["prompts", "Promptlar"],
              ["responses", "Cevaplar"],
              ["tools", "Araçlar"],
              ["system", "Sistem & bağlam"],
              ["all", "Tüm kayıt"],
              ["summary", "Özet"],
            ] as [ViewMode, string][]
          ).map(([mode, label]) => (
            <button
              key={mode}
              role="tab"
              aria-selected={viewMode === mode}
              className={viewMode === mode ? "active" : ""}
              onClick={() => onViewMode(mode)}
            >
              {label}
            </button>
          ))}
          {search && <span className="exact-filter">Tam eşleşme · “{search}”</span>}
        </div>
      </header>

      <div className="inspector-body">
        <Transcript session={data} mode={viewMode} search={search} loading={turnLoading} offset={turnOffset} hasMore={detail.has_more} total={detail.total_turns} loadingMore={loadingMore} jumpLoading={jumpLoading} onLoadMore={onLoadMore} onLoadPrevious={onLoadPrevious} onJump={onJump} />
        <EvidencePanel cost={cost} session={data.session} files={files} filesLoading={filesLoading} open={evidenceOpen} onClose={() => setEvidenceOpen(false)} onNotice={onNotice} />
      </div>
    </section>
  );
}

function Transcript({ session, mode, search, loading, offset, hasMore, total, loadingMore, jumpLoading, onLoadMore, onLoadPrevious, onJump }: { session: SessionDetail["data"]; mode: ViewMode; search: string; loading: boolean; offset: number; hasMore: boolean; total: number; loadingMore: boolean; jumpLoading: boolean; onLoadMore: () => void; onLoadPrevious: () => void; onJump: (edge: "start" | "end") => Promise<void> }) {
  const parentRef = useRef<HTMLDivElement>(null);
  const spineRef = useRef<HTMLDivElement>(null);
  const loadRef = useRef<HTMLDivElement>(null);
  const pendingJump = useRef<"start" | "end" | null>(null);
  const positionedSession = useRef<string | null>(null);
  const turns = useMemo(() => {
    return session.turns;
  }, [session.turns, mode]);
  const virtualizer = useVirtualizer({
    count: mode === "summary" ? 0 : turns.length,
    getScrollElement: () => parentRef.current,
    getItemKey: (index) => `${turns[index].ordinal}-${turns[index].external_id || "turn"}`,
    estimateSize: (index) => Math.min(420, 112 + turns[index].text.length * 0.09 + turns[index].tool_calls.length * 72),
    overscan: 5,
  });

  useEffect(() => {
    const positionKey = `${session.session.id}:${mode}:${search}`;
    if (positionedSession.current === positionKey || !turns.length) return;
    positionedSession.current = positionKey;
    pendingJump.current = offset > 0 ? "end" : "start";
  }, [session.session.id, mode, search, offset, turns.length]);

  useEffect(() => {
    const edge = pendingJump.current;
    const root = parentRef.current;
    if (!edge || !root) return;
    pendingJump.current = null;
    if (edge === "start") {
      root.scrollTo({ top: 0, behavior: "auto" });
      return;
    }

    // Markdown, highlighted code and measured virtual rows can grow after the
    // page arrives. Keep the viewport pinned until those late measurements
    // settle instead of targeting a stale scrollHeight once.
    let frame = 0;
    const pinToBottom = () => {
      window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(() => {
        root.scrollTop = root.scrollHeight;
        virtualizer.scrollToIndex(Math.max(0, turns.length - 1), { align: "end" });
      });
    };
    pinToBottom();
    const observer = new ResizeObserver(pinToBottom);
    if (spineRef.current) observer.observe(spineRef.current);
    const timeout = window.setTimeout(() => {
      observer.disconnect();
      pinToBottom();
    }, 1400);
    return () => {
      observer.disconnect();
      window.clearTimeout(timeout);
      window.cancelAnimationFrame(frame);
    };
  }, [turns, virtualizer]);

  const jump = async (edge: "start" | "end") => {
    pendingJump.current = edge;
    await onJump(edge);
  };

  useEffect(() => {
    const root = parentRef.current;
    const target = loadRef.current;
    if (!root || !target || !hasMore || loadingMore) return;
    const observer = new IntersectionObserver(
      ([entry]) => entry.isIntersecting && onLoadMore(),
      { root, rootMargin: "500px 0px" },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [hasMore, loadingMore, onLoadMore, turns.length]);

  if (mode === "summary") {
    return (
      <div className="transcript-scroll summary-view" key="summary">
        <div className="summary-title-row">
          <FileText size={20} />
          <h3>Oturum özeti</h3>
          <CopyAction value={session.summary || ""} title="Özeti kopyala" />
        </div>
        <p>{session.summary || "Bu kaynağın içinde kayıtlı bir özet bulunmuyor."}</p>
        <dl className="summary-facts">
          <div><dt>Tur</dt><dd>{session.session.turn_count}</dd></div>
          <div><dt>Araç çağrısı</dt><dd>{session.session.tool_call_count}</dd></div>
          <div><dt>Sağlayıcı</dt><dd>{providerMeta[session.session.provider].label}</dd></div>
        </dl>
      </div>
    );
  }

  if (loading) {
    return <div className="transcript-scroll transcript-loading"><div className="loading-rule" /><div className="loading-block wide" /><div className="loading-block" /></div>;
  }

  if (total === 0) {
    return (
      <div className="transcript-scroll inspector-empty transcript-empty">
        {search ? <Search size={20} /> : <Archive size={20} />}
        <h2>{search ? "Bu konuşmada tam eşleşme yok" : "Yerel konuşma metni bulunamadı"}</h2>
        <p>{search ? `“${search}” metni bu görünümde birebir bulunamadı. Aramayı temizleyin veya başka bir konuşma seçin.` : "Bu kayıt yalnızca başlık ve oturum metadata’sı bırakmış. Uygulama bunu konuşma varmış gibi göstermiyor; tam metin için sağlayıcının resmi veri dışa aktarımı gerekebilir."}</p>
      </div>
    );
  }

  return (
    <div className="transcript-scroll" ref={parentRef} key={mode}>
      <nav className="transcript-jumps" aria-label="Konuşmada hızlı gezinme">
        <button onClick={() => void jump("start")} disabled={jumpLoading} title="Konuşmanın en başına git"><ArrowUp size={15} /><span>En üst</span></button>
        <button onClick={() => void jump("end")} disabled={jumpLoading} title="Konuşmanın en sonuna git"><ArrowDown size={15} /><span>En alt</span></button>
      </nav>
      {offset > 0 && (
        <div className="transcript-progress previous-records" role="status">
          <span>{formatNumber(offset)} daha eski kayıt var</span>
          <button className="load-more-button" onClick={onLoadPrevious} disabled={loadingMore}>
            {loadingMore ? "Önceki bölüm yükleniyor…" : `Önceki ${formatNumber(Math.min(PAGE_SIZE, offset))} kaydı yükle`}
          </button>
        </div>
      )}
      <div className="turn-spine" ref={spineRef} style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const turn = turns[virtualRow.index];
          return (
            <div
              key={`${turn.ordinal}-${turn.external_id || "turn"}`}
              ref={virtualizer.measureElement}
              data-index={virtualRow.index}
              className="virtual-turn"
              style={{ transform: `translateY(${virtualRow.start}px)` }}
            >
              <TurnRecord turn={turn} sessionId={session.session.id} projectPath={session.session.project_path} />
            </div>
          );
        })}
      </div>
      <div className="transcript-progress" ref={loadRef} role="status">
        <span>{total ? `${formatNumber(offset + 1)}–${formatNumber(offset + turns.length)} / ${formatNumber(total)} kayıt` : "Kayıt yok"}</span>
        {hasMore && (
          <button className="load-more-button" onClick={onLoadMore} disabled={loadingMore}>
            {loadingMore ? "Sonraki bölüm yükleniyor…" : `Sonraki ${formatNumber(Math.min(PAGE_SIZE, total - offset - turns.length))} kaydı yükle`}
          </button>
        )}
      </div>
    </div>
  );
}

function TurnRecord({ turn, sessionId, projectPath }: { turn: Turn; sessionId: string; projectPath?: string }) {
  const copyTitle = turn.role === "user" ? "Promptu kopyala" : turn.role === "assistant" ? "Yanıtı kopyala" : "Kaydı kopyala";
  return (
    <article className={`turn-record ${turn.role}`}>
      <div className="turn-index" title={`Kaynak kayıt ${turn.ordinal + 1}`}>{turn.prompt_ordinal ? `T${String(turn.prompt_ordinal).padStart(3, "0")}` : "—"}</div>
      <div className="turn-sheet">
        <header>
          <strong>{roleLabel(turn.role)}</strong>
          <span className="turn-meta" title={turn.created_at}>{formatTurnTimestamp(turn.created_at)}</span>
          {turn.usage?.total_tokens && <span className="turn-meta">{formatNumber(turn.usage.total_tokens)} tok.</span>}
          {turn.text.trim() && <CopyAction className="turn-copy-button" value={cleanDisplayText(turn.text)} title={copyTitle} />}
        </header>
        {turn.text && <Suspense fallback={<div className="markdown-placeholder">Metin hazırlanıyor…</div>}><MarkdownBody source={turn.text} basePath={projectPath} /></Suspense>}
        {turn.tool_calls.map((tool, index) => (
          <ToolRecord key={tool.external_id || `${tool.name}-${index}`} tool={tool} sessionId={sessionId} turnOrdinal={turn.ordinal} toolOrdinal={index} projectPath={projectPath} />
        ))}
      </div>
    </article>
  );
}

function ToolRecord({ tool, sessionId, turnOrdinal, toolOrdinal, projectPath }: { tool: ToolCall; sessionId: string; turnOrdinal: number; toolOrdinal: number; projectPath?: string }) {
  const [open, setOpen] = useState(false);
  const [detail, setDetail] = useState<ToolCall>(tool);
  const [detailLoading, setDetailLoading] = useState(false);
  const toggle = async () => {
    const next = !open;
    setOpen(next);
    if (!next || detail.arguments_json != null || detail.result_text != null || detailLoading) return;
    setDetailLoading(true);
    try {
      const loaded = await getToolCall(sessionId, turnOrdinal, toolOrdinal);
      if (loaded) setDetail(loaded);
    } finally {
      setDetailLoading(false);
    }
  };
  const completeRecord = [detail.arguments_json, detail.result_text].filter(Boolean).join("\n\n");
  return (
    <div className="tool-record">
      <button className="tool-summary" onClick={() => void toggle()} aria-expanded={open}>
        <Wrench size={14} />
        <strong>{tool.name}</strong>
        <span className={`tool-status ${tool.status === "error" ? "error" : ""}`}>{tool.status || "recorded"}</span>
        {tool.duration_ms != null && <small>{formatDuration(tool.duration_ms)}</small>}
        <ChevronDown size={14} className={open ? "rotated" : ""} />
      </button>
      {open && (
        <div className="tool-detail">
          <div className="tool-detail-toolbar">
            <span>Araç kaydı</span>
            <CopyAction value={completeRecord} title="Tüm araç kaydını kopyala" label="Tümünü kopyala" />
          </div>
          {detailLoading && <div className="tool-loading">Araç ayrıntısı yükleniyor…</div>}
          {detail.arguments_json && (
            <section className="tool-detail-section">
              <header><label>Arguments</label><CopyAction value={detail.arguments_json} title="Araç argümanlarını kopyala" /></header>
              <pre><Suspense fallback={<code>{detail.arguments_json}</code>}><HighlightedJson source={detail.arguments_json} /></Suspense></pre>
            </section>
          )}
          {detail.result_text && (
            <section className="tool-detail-section">
              <header><label>Result</label><CopyAction value={detail.result_text} title="Araç sonucunu kopyala" /></header>
              <div className="tool-result"><Suspense fallback={<div className="markdown-placeholder">Sonuç hazırlanıyor…</div>}><MarkdownBody source={detail.result_text} basePath={projectPath} /></Suspense></div>
            </section>
          )}
        </div>
      )}
    </div>
  );
}

function EvidencePanel({ cost, session, files, filesLoading, open, onClose, onNotice }: { cost: CostEstimate; session: SessionListItem; files: FileReference[]; filesLoading: boolean; open: boolean; onClose: () => void; onNotice: (message: string) => void }) {
  const confidenceLabel = cost.confidence === "observed" ? "Kaydedilmiş" : cost.confidence === "reconstructed" ? "Yeniden kuruldu" : "Tahmini";
  const [fileFilter, setFileFilter] = useState<"user" | "assistant" | "tool" | "all">("user");
  const [collectionOrigin, setCollectionOrigin] = useState<"user" | "assistant" | "all">("user");
  const [collecting, setCollecting] = useState(false);
  const [lastCollection, setLastCollection] = useState<FileCollectionReport | null>(null);
  useEffect(() => {
    setFileFilter("user");
    setCollectionOrigin("user");
    setLastCollection(null);
  }, [session.id]);
  const filteredFiles = fileFilter === "all" ? files : files.filter((file) => file.origins.includes(fileFilter));
  const userFiles = filteredFiles.filter((file) => file.origins.includes("user"));
  const modelFiles = filteredFiles.filter((file) => !file.origins.includes("user"));
  const openFile = async (path: string) => {
    try {
      const opened = await revealPath(path);
      if (opened !== path) onNotice(`Açıldı · ${opened}`);
    } catch (error) {
      onNotice(String(error));
    }
  };
  const collectFiles = async () => {
    try {
      const selected = await openDialog({ directory: true, multiple: false, title: "Dosya paketinin kaydedileceği klasör" });
      if (typeof selected !== "string") return;
      setCollecting(true);
      const report = await collectSessionFiles(session.id, selected, collectionOrigin);
      setLastCollection(report);
      onNotice(`${report.copied_files} dosya toplandı · ${report.duplicates} tekrar tekilleştirildi · ${report.missing} eksik`);
    } catch (error) {
      onNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setCollecting(false);
    }
  };
  return (
    <aside className={open ? "evidence-panel open" : "evidence-panel"} aria-label="Oturum ölçümleri">
      <button className="evidence-close" onClick={onClose} aria-label="Kayıt dökümünü kapat"><X size={15} /></button>
      <div className="evidence-heading">
        <span>Kayıt dökümü</span>
        <span className={`confidence ${cost.confidence}`}>
          <i /> {confidenceLabel}
        </span>
      </div>
      <div className="cost-figure">
        <span>API karşılığı</span>
        <strong>{cost.amount_usd == null ? "—" : formatCurrency(cost.amount_usd)}</strong>
        <small>{cost.catalog_model || "Fiyat eşleşmesi yok"}</small>
      </div>
      <dl className="usage-ledger">
        <div><dt>Input</dt><dd>{formatNumber(cost.input_tokens)}</dd></div>
        <div><dt>Output</dt><dd>{formatNumber(cost.output_tokens)}</dd></div>
        <div><dt>Cached</dt><dd>{formatNumber(cost.cached_input_tokens)}</dd></div>
        <div><dt>Cache write</dt><dd>{formatNumber(cost.cache_write_input_tokens)}</dd></div>
        <div><dt>Reasoning</dt><dd>{formatNumber(cost.reasoning_tokens)}</dd></div>
      </dl>
      <div className="evidence-rule" />
      <dl className="record-ledger">
        <div><dt>Konuşma turu</dt><dd>{session.turn_count}</dd></div>
        <div><dt>Araç</dt><dd>{session.tool_call_count}</dd></div>
        <div><dt>Durum</dt><dd>{session.archived ? "Arşiv" : "Aktif"}</dd></div>
        {session.turn_count === 0 && <div><dt>İçerik</dt><dd>Yalnız metadata</dd></div>}
        <div><dt>Fiyat tarihi</dt><dd>{cost.pricing_date || "—"}</dd></div>
      </dl>
      <p className="cost-note">
        <AlertTriangle size={14} /> {cost.note}
      </p>
      <div className="source-reference-section">
        <div className="file-reference-heading">
          <span>Yerel köken</span>
          <small>salt okunur</small>
        </div>
        {session.source_path && (
          <PathReference
            icon={<Database size={14} />}
            label="Oturum kaynak dosyası"
            path={session.source_path}
            onOpen={openFile}
          />
        )}
        {session.project_path && (
          <PathReference
            icon={<Folder size={14} />}
            label="Çalışma alanı"
            path={session.project_path}
            onOpen={openFile}
          />
        )}
      </div>
      <div className="file-reference-section">
        <div className="file-reference-heading">
          <span>Etiketlenen dosyalar</span>
          <small>{filesLoading ? "aranıyor" : `${filteredFiles.length}/${files.length}`}</small>
        </div>
        {!filesLoading && files.length > 0 && (
          <div className="file-filter-strip" aria-label="Dosya kaynağı filtresi">
            {([['user', 'Sen'], ['assistant', 'Asistan'], ['tool', 'Araç'], ['all', 'Tümü']] as const).map(([id, label]) => (
              <button key={id} className={fileFilter === id ? "active" : ""} onClick={() => setFileFilter(id)}>{label}<small>{id === "all" ? files.length : files.filter((file) => file.origins.includes(id)).length}</small></button>
            ))}
          </div>
        )}
        {filesLoading ? (
          <div className="file-reference-loading">Dosya referansları taranıyor…</div>
        ) : filteredFiles.length ? (
          <div className="file-reference-groups">
            {fileFilter === "all" ? <>
              <FileReferenceGroup label="Senin etiketlediklerin" files={userFiles} onOpen={openFile} />
              <FileReferenceGroup label="Model ve araç kayıtları" files={modelFiles} onOpen={openFile} />
            </> : (
              <FileReferenceGroup label={fileFilter === "user" ? "Senin etiketlediklerin" : fileFilter === "assistant" ? "Asistanın etiketledikleri" : "Araç kayıtları"} files={filteredFiles} onOpen={openFile} />
            )}
          </div>
        ) : (
          <p className="file-reference-empty">Bu kaynakta dosya referansı bulunamadı.</p>
        )}
      </div>
      <div className="file-collection-section">
        <div className="file-reference-heading"><span>Dosya paketi</span><small>raporlu kopya</small></div>
        <p>Yalnızca konuşmada @ ile açıkça belirtilen veya sohbete eklenen dosyaları kopyalar. Aynı fiziksel dosya yalnız bir kez alınır; eksikler JSON raporuna yazılır.</p>
        <div className="file-filter-strip collection-origin-strip" aria-label="Dosya paketine dahil edilecek kaynak">
          {([['user', 'Sen'], ['assistant', 'Asistan'], ['all', 'Tümü']] as const).map(([id, label]) => (
            <button key={id} className={collectionOrigin === id ? "active" : ""} onClick={() => setCollectionOrigin(id)}>{label}<small>{id === "all" ? "Sen + Asistan" : ""}</small></button>
          ))}
        </div>
        <div className="file-collection-actions">
          <button className="secondary-button" disabled={collecting} onClick={() => void collectFiles()}><FolderOutput size={14} /> {collecting ? "Toplanıyor…" : "Topla…"}</button>
        </div>
        {lastCollection && (
          <button className="collection-result" onClick={() => void openFile(lastCollection.destination)} title={lastCollection.destination}>
            <Check size={14} /><span><strong>Son paket hazır</strong><small>{lastCollection.copied_files} dosya · klasörde göster</small></span>
          </button>
        )}
        <small>“Topla” önce hedef konumu sorar; seçimi iptal edersen hiçbir işlem başlamaz.</small>
      </div>
      <div className="provenance-seal">
        <ShieldCheck size={17} />
        <span>Kaynak dosya değişmeden okundu</span>
      </div>
    </aside>
  );
}

function FileReferenceGroup({ label, files, onOpen }: { label: string; files: FileReference[]; onOpen: (path: string) => Promise<void> }) {
  if (!files.length) return null;
  return (
    <section className="file-reference-group">
      <header><span>{label}</span><small>{files.length}</small></header>
      <div className="file-reference-list">
        {files.map((file) => (
          <button key={file.path} className={file.exists ? "file-reference" : "file-reference missing"} onClick={() => void onOpen(file.path)} title={file.exists ? file.path : `Silinmiş dosya · ${file.path}`}>
            {file.is_image && !file.exists ? <ImageOff size={14} /> : <FileText size={14} />}
            <span>{file.path.split(/[\\/]/).filter(Boolean).at(-1) || file.path}</span>
            <small>{file.exists ? "Klasörde göster" : "Silinmiş · önceki konumu göster"}</small>
            <span className="file-origin-row">
              {file.origins.map((origin) => <i key={origin}>{fileOriginLabel(origin)}</i>)}
            </span>
          </button>
        ))}
      </div>
    </section>
  );
}

function fileOriginLabel(origin: FileReference["origins"][number]) {
  return { user: "Sen", assistant: "Asistan", tool: "Araç", system: "Sistem", unknown: "Kayıt" }[origin];
}

function PathReference({ icon, label, path, onOpen }: { icon: ReactNode; label: string; path: string; onOpen: (path: string) => Promise<void> }) {
  return (
    <div className="source-reference">
      <button className="source-reference-open" onClick={() => void onOpen(path)} title={`${path} · klasörde göster`}>
        {icon}
        <span>{label}</span>
        <small>{path}</small>
      </button>
      <CopyAction value={path} title={`${label} yolunu kopyala`} />
    </div>
  );
}

function roleLabel(role: Turn["role"]) {
  return { user: "Sen", assistant: "Asistan", system: "Sistem", tool: "Araç", reasoning: "Reasoning", unknown: "Kayıt" }[role];
}

function leafPath(path?: string) {
  return path?.split(/[\\/]/).filter(Boolean).at(-1);
}

function formatDate(value?: string, long = false) {
  if (!value) return "Tarih yok";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value.slice(0, long ? 19 : 10);
  return new Intl.DateTimeFormat("tr-TR", long
    ? { day: "numeric", month: "long", year: "numeric", hour: "2-digit", minute: "2-digit", timeZone: "Europe/Istanbul" }
    : { day: "numeric", month: "short", timeZone: "Europe/Istanbul" }).format(date);
}

function formatTurnTimestamp(value?: string) {
  if (!value) return "";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat("tr-TR", {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
    timeZone: "Europe/Istanbul",
  }).format(date).replace(",", " ·");
}

function formatNumber(value: number) {
  return new Intl.NumberFormat("tr-TR", { notation: value > 9999 ? "compact" : "standard", maximumFractionDigits: 1 }).format(value);
}

function formatCurrency(value: number) {
  return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", minimumFractionDigits: value < 0.01 ? 4 : 2, maximumFractionDigits: value < 0.01 ? 4 : 2 }).format(value);
}

function formatDuration(value: number) {
  return value > 999 ? `${(value / 1000).toFixed(1)}s` : `${value}ms`;
}

function safeFileName(value: string) {
  return value.replace(/[<>:"/\\|?*\u0000-\u001F]/g, "-").slice(0, 90) || "conversation";
}
