import { useMemo, useState } from "react";
import { CircleDollarSign, Plus, RotateCcw } from "lucide-react";
import { defaultPrices, withUnknownModels } from "./prices";
import type { PriceSetting, Provider, SessionListItem } from "./types";

const labels: Record<Provider, string> = { codex: "Codex", claude: "Claude", grok: "Grok", antigravity: "AGY" };

export function SettingsView({ prices, sessions, onChange }: { prices: PriceSetting[]; sessions: SessionListItem[]; onChange: (prices: PriceSetting[]) => void }) {
  const rows = useMemo(() => withUnknownModels(prices, sessions), [prices, sessions]);
  const [provider, setProvider] = useState<Provider>("codex");
  const [pattern, setPattern] = useState("");
  const update = (id: string, field: keyof PriceSetting, value: string) => {
    const numeric = ["input_per_million_usd", "cached_input_per_million_usd", "cache_write_per_million_usd", "output_per_million_usd"].includes(field);
    const base = prices.some((entry) => entry.id === id) ? prices : rows;
    onChange(base.map((entry) => entry.id === id ? { ...entry, [field]: numeric ? (value === "" ? undefined : Number(value)) : value } : entry));
  };
  const add = () => {
    const clean = pattern.trim();
    if (!clean) return;
    const entry: PriceSetting = { id: `${provider}:${clean.toLowerCase()}`, provider, model_pattern: clean, catalog_model: clean, built_in: false };
    onChange([...rows.filter((row) => row.id !== entry.id), entry]);
    setPattern("");
  };
  return (
    <section className="insight-surface settings-view">
      <header className="surface-heading">
        <div><h1>API fiyatları</h1><p>Kayıtlı tokenları API karşılığına çevirmek için kullanılan yerel katalog.</p></div>
        <button className="secondary-button" onClick={() => onChange(defaultPrices)}><RotateCcw size={14} /> Varsayılanlar</button>
      </header>
      <div className="pricing-note"><CircleDollarSign size={17} /><p>Fiyatlar milyon token başınadır. Cache okuma ve kayıtta bulunan cache yazma ayrı hesaplanır; reasoning tokenları sağlayıcının output sayacındaki haliyle ücretlendirilir. Kaydedilmeyen tool ücreti, cache saklama ve uzun-context farkı tahmine eklenmez.</p></div>
      <div className="price-table">
        <div className="price-head"><span>Model eşleşmesi</span><span>Input</span><span>Cache read</span><span>Cache write</span><span>Output</span><span>Tarih</span></div>
        {rows.map((entry) => (
          <div className={`price-row ${entry.input_per_million_usd == null || entry.output_per_million_usd == null ? "missing" : ""}`} key={entry.id}>
            <div className="price-model"><span className={`usage-dot ${entry.provider}`} /><strong>{entry.catalog_model}</strong><small>{labels[entry.provider]} · {entry.model_pattern}{entry.built_in ? " · katalog" : " · özel"}</small></div>
            <input aria-label={`${entry.catalog_model} input fiyatı`} type="number" min="0" step="0.01" value={entry.input_per_million_usd ?? ""} placeholder="—" onChange={(event) => update(entry.id, "input_per_million_usd", event.target.value)} />
            <input aria-label={`${entry.catalog_model} cached input fiyatı`} type="number" min="0" step="0.01" value={entry.cached_input_per_million_usd ?? ""} placeholder="—" onChange={(event) => update(entry.id, "cached_input_per_million_usd", event.target.value)} />
            <input aria-label={`${entry.catalog_model} cache write fiyatı`} type="number" min="0" step="0.01" value={entry.cache_write_per_million_usd ?? ""} placeholder="—" onChange={(event) => update(entry.id, "cache_write_per_million_usd", event.target.value)} />
            <input aria-label={`${entry.catalog_model} output fiyatı`} type="number" min="0" step="0.01" value={entry.output_per_million_usd ?? ""} placeholder="—" onChange={(event) => update(entry.id, "output_per_million_usd", event.target.value)} />
            <input aria-label={`${entry.catalog_model} fiyat tarihi`} value={entry.effective_date ?? ""} placeholder="YYYY-AA-GG" onChange={(event) => update(entry.id, "effective_date", event.target.value)} />
          </div>
        ))}
      </div>
      <div className="add-price-row">
        <select value={provider} onChange={(event) => setProvider(event.target.value as Provider)}>{Object.entries(labels).map(([id, label]) => <option key={id} value={id}>{label}</option>)}</select>
        <input value={pattern} onChange={(event) => setPattern(event.target.value)} placeholder="Yeni model veya eşleşme kalıbı" onKeyDown={(event) => { if (event.key === "Enter") add(); }} />
        <button className="secondary-button" onClick={add}><Plus size={14} /> Model ekle</button>
      </div>
    </section>
  );
}
