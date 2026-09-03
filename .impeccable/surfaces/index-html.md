---
version: 1
slug: "index-html"
primary_target: "index.html"
related_targets: ["src/App.tsx","src/styles.css"]
---

# Contextractor desktop archive

- Scope: the primary desktop application shell and session inspector in `index.html` and `src/`.
- Visitor mode: Operate.
- Audience: a frequent multi-provider AI user working at a desktop with a large local session archive.
- Job: discover local sources, understand import health, search sessions, inspect a normalized transcript, compare observed versus estimated usage, and export a selected slice.
- Required actions: rescan, filter, select a conversation, change transcript view, inspect tool calls, and export full history, prompts, summary, JSON, or JSONL.
- Required proof: real provider discovery and session counts from the local Rust core; provenance and confidence remain visible near every metric.
- Constraints: local-only and read-only; keyboard operable; no decorative AI-dashboard tropes, glass cards, glowing gradients, fake activity, or invented claims.
- Chosen direction: Conservation Light Table. A cool limestone workspace holds translucent evidence strips and graphite specimen labels; cobalt is reserved for active provenance and verification. The three-pane shell behaves like an archival workbench rather than a grid of metric cards.
- Memorable moment: after a scan, source strips settle into the left rail while the selected conversation's turn spine resolves from raw provider events into a clean reading transcript.
- Signature interaction: changing transcript mode masks and reflows the same turn spine in place, preserving scroll context rather than navigating to a different page.
- Unresolved decisions: none; the user delegated product and visual decisions.
