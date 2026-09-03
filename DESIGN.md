# Contextractor design note

## Direction

Contextractor is a conservation light table for personal AI records. The interface should feel like a careful reading instrument: cool limestone ground, white paper surfaces, graphite labels, and one cobalt provenance accent. It is deliberately not a generic analytics dashboard.

## Information architecture

The first viewport is a three-pane archive workbench:

1. Providers and source health.
2. Searchable session catalog.
3. The selected conversation's transcript plus evidence ledger.

The same turn spine reflows when the user switches between all turns, prompts, tools, and summary. This keeps navigation spatially stable and makes “API karşılığı” an evidence label, not a fake accounting number.

Usage and API pricing live as two focused ledgers in the provider rail. Usage answers behavioral questions without turning the archive into a generic dashboard; pricing keeps dated defaults and missing-model overrides auditable. On narrower windows the evidence ledger becomes an explicit drawer so the transcript remains readable.

## Visual rules

- Archivo Variable is the only UI typeface; monospace is reserved for tool names and JSON.
- Cool limestone (`#e9ece7`) is the ground, paper (`#fbfcf8`) is the reading surface, graphite (`#171b19`) is the ink, and cobalt (`#2854d6`) means selected/provenance state.
- Rules are hairline and structural. Shadows are used only to lift the active sheet or native menu.
- No gradients, glass panels, decorative illustrations, auto-rotating content, or card grids.
- Keyboard focus, reduced motion, and empty/loading/error states are first-class.
- Conversation Markdown is rendered as restrained editorial typography. Fenced code and tool JSON use a dedicated evidence palette while exports retain the original source text.

## Data ethics

The app reads source files only. Every imported turn keeps provider/session lineage and usage confidence. API cost is a dated catalog comparison and carries a caveat when the source lacks observed token counts. Tool results can contain secrets or command output, so exports and the SQLite archive are treated as sensitive local data.

## Motion

There is one signature motion: switching transcript modes resolves the same paper-like turn spine into the new view with a short clip/opacity transition. Scanning uses a functional spinner and progress bar. `prefers-reduced-motion` disables both.

## Asset provenance

`src/assets/contextractor-mark.svg` is authored in-repository as a geometric archive mark. Tauri's icon command generated the platform icon derivatives from that SVG; no generated or third-party imagery is used in the UI.
