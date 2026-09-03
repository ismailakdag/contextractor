# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Tauri 2 desktop shell with a React and TypeScript interface, a Rust ingestion core, and a local SQLite database. The user approved this recommended stack. Distribution targets are Windows, macOS, and Linux, with portable packaging where the operating system permits it.

## Users

The primary user is a frequent user of multiple AI desktop and agent applications who wants to recover, inspect, export, and learn from locally stored conversation history without manually locating each application's private data format.

## Product Purpose

Contextractor discovers supported AI applications on the current computer, imports their local sessions into a normalized private database, and lets the user inspect prompts, responses, tool calls, token usage, estimated API cost, and longer-term LLM usage patterns. Success means a user can open the application, see recoverable sessions across providers, understand the provenance and confidence of every metric, and export the exact slice of history they need.

## Positioning

The product treats heterogeneous on-device AI session stores as versioned evidence sources. Provider-specific read-only adapters normalize them into one auditable timeline while preserving links back to the original records and explicitly separating observed usage from reconstructed or estimated metrics.

## Operating Context

The application runs locally against session stores created by Codex, Claude Code and Claude Desktop local-agent features, Grok CLI, Antigravity and AGY, and later other desktop AI tools. Sources may be JSON, JSONL, SQLite, LevelDB, protobuf text, or provider export archives; some files can be live, locked, incomplete, duplicated, archived, or changed by application updates.

## Capabilities and Constraints

- Startup discovery of known provider locations on Windows, macOS, and Linux.
- Read-only, incremental imports into a normalized SQLite database.
- Conversation views for full history, user prompts only, summaries, tool calls, artifacts, and token or cost analysis.
- Export of one conversation, one provider, a filtered selection, or the full archive in portable formats.
- Exact token usage when recorded; reconstructed or tokenizer-based estimates otherwise, always labeled with confidence and method.
- API-equivalent cost estimates use a dated, replaceable model-pricing catalog and never present an estimate as an invoice.
- Source credentials, cookies, API keys, OAuth material, and authentication databases are out of scope and must never be imported.
- Cloud-only consumer chat history is supported only through an official export or authorized API when no reliable local canonical transcript exists.
- Provider formats are private implementation details and can change, so adapters must be independently versioned and fail safely on unsupported schemas.
- The first MVP is local-only and includes the four locally verified families: Codex, Claude Code/Desktop local-agent bridge, Grok CLI, and Antigravity/AGY.

## Brand Commitments

The user delegated visual decisions and asked for a minimalist, premium-feeling interface with no generic AI-generated dashboard aesthetic. Function and information clarity take precedence over decorative novelty.

## Evidence on Hand

A read-only schema scan on the development machine confirmed local session stores for Claude Code, Codex, Grok CLI, Antigravity/AGY, and Continue. The confirmed MVP sources contain structured user and assistant messages, tool calls or results, model metadata, summaries, and—in some formats—recorded token usage. No brand assets, testimonials, commercial claims, or final product name have been supplied.

## Product Principles

- Local and private by default.
- Never mutate or lock a provider's source data.
- Preserve provenance; every normalized record can explain where it came from.
- Prefer honest confidence labels over false precision.
- Add providers through isolated adapters rather than weakening the common data model.
- Make export and user ownership first-class, not an afterthought.

## Accessibility & Inclusion

The desktop interface must be keyboard operable, work at common desktop scaling levels, expose semantic labels and focus states, and avoid using color as the sole indicator of provider, status, or confidence.
