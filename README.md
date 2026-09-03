# Contextractor

Contextractor, masaüstü AI araçlarının yerelde bıraktığı oturum kayıtlarını tek bir özel arşivde birleştirir. Kaynak dosyaları salt okunur açar; konuşmaları, promptları, araç çağrılarını ve mevcut token ölçümlerini normalize ederek SQLite'a yazar.

## İlk sürümde desteklenen kaynaklar

| Sağlayıcı | Okunan yerel kayıt | Not |
| --- | --- | --- |
| Codex | `~/.codex/sessions/**/*.jsonl`, `~/.codex/archived_sessions/*.jsonl` | Aktif ve arşivlenmiş task kayıtları |
| Claude Code | `~/.claude/projects/**/*.jsonl` | Subagent günlükleri ana konuşmadan ayrı içe alınmaz |
| Claude Desktop | Uygulamanın `claude-code-sessions` metadata köprüsü | Yalnızca yereldeki Claude Code kaydına bağlanabilen içerik |
| Grok CLI | `~/.grok/sessions/**/chat_history.jsonl` | grok.com tüketici sohbetleri buluttadır; tarayıcı kimlik bilgileri okunmaz |
| Antigravity / AGY | `~/.gemini/antigravity/brain/**/transcript_full.jsonl` | Planner cevapları ve araç olayları dahil |

Uygulama açılışta keşif ve artımlı tarama yapar. Sonraki taramalarda dosya parmak izi değişmeyen kayıtlar tekrar parse edilmez.

## Özellikler

- Sağlayıcı, proje ve tarih bağlamıyla birleşik oturum kataloğu
- Prompt, cevap ve araç çağrılarında indeksli tam metin arama
- Tüm akış, yalnız promptlar, system promptları, cevaplar, araç çağrıları ve özet görünümleri
- Konuşma içindeki Markdown'ı güvenli biçimde render etme; kod bloklarında ve araç JSON'larında syntax highlighting
- Markdown, JSON, JSONL, kullanıcı promptları, system promptları, birleşik context, cevaplar, araç çağrıları ve özet dışa aktarma
- Sağlayıcı bazında oturum, prompt, araç, token, ortalama kullanım ve en yoğun gün analizi
- Kaydedilmiş token sayaçları; yoksa açıkça **tahmini** olarak işaretlenen yaklaşık sayaçlar
- Tarihli API fiyat kataloğuyla “API'de olsaydı” maliyet karşılığı; bilinmeyen modeller için kullanıcı tarafından düzenlenebilir oranlar
- Büyük JSONL kayıtlarını belleğe bütünüyle almayan streaming parser ve sayfalı transcript
- Arka planda çalışan arama/okuma komutları, toplu SQL sorguları ve büyük Claude oturumlarında sabit boyutlu ilk yükleme
- İnternet, telemetry veya gizli browser credential erişimi yok

## Çalıştırma

Gereksinimler: Node.js 20+, güncel kararlı Rust ve Tauri'nin işletim sistemi bağımlılıkları.

```powershell
npm install
npm run tauri dev
```

Web arayüzünü örnek veriyle açmak için:

```powershell
npm run dev
```

Test ve üretim derlemesi:

```powershell
cargo test --workspace
npm run build
npm run tauri build
```

## Portable kullanım

Paketlenmiş uygulama dosyasının yanına boş bir `portable.flag` dosyası koyun. Contextractor bu durumda veritabanını aynı klasördeki `data/contextractor.sqlite` konumuna yazar. Bayrak yoksa işletim sisteminin standart uygulama veri dizini kullanılır.

Portable klasörü taşınabilir; ancak kaynak AI oturumları oraya kopyalanmaz. Arşiv veritabanı prompt ve cevap içeriği taşıdığı için klasörü hassas veri olarak değerlendirin. API fiyat ayarları şu anda cihazdaki WebView profilinde saklanır; portable veritabanına dahil edilmez.

## Veri ve güvenlik modeli

- Kaynak dosyalar hiçbir zaman değiştirilmez veya silinmez.
- SQLite arşivi yalnızca yerel makinede tutulur.
- Tool result alanları veritabanında ve dışa aktarmalarda bulunabilir; bunlar komut çıktısı, yol veya secret içerebilir.
- Maliyet hesabı abonelik ücretini taklit etmez. Eşleşen modelin API input/output/cache fiyatını uygular; bölge, uzun context, özel tool ücreti ve indirimleri hesaba katmaz.
- Kaydedilmemiş token miktarı karakter sayısı üzerinden yaklaşık hesaplanır ve arayüzde tahmini olarak etiketlenir.

## Mimari

- `crates/contextractor-core`: keşif, provider parser'ları, normalize model, SQLite/FTS5, export ve fiyatlama
- `crates/contextractor-cli`: headless keşif, tarama, listeleme ve export
- `src-tauri`: native masaüstü komutları ve portable veri konumu
- `src`: React arayüzü

Yeni bir sağlayıcı eklemek için önce salt-okunur `SourceCandidate` keşfi, ardından `ParsedSession` üreten streaming parser eklenir. Kaynağın disk şeması sürümlenmemişse parser toleranslı olmalı ve anlayamadığı event'i uydurmamalıdır.
