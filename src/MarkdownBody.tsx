import { Children, isValidElement, memo, useMemo, useState, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import css from "highlight.js/lib/languages/css";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import markdown from "highlight.js/lib/languages/markdown";
import powershell from "highlight.js/lib/languages/powershell";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import { FileImage, ImageOff } from "lucide-react";
import { revealPath } from "./bridge";
import { CopyAction } from "./CopyAction";
import { cleanDisplayText } from "./text";

hljs.registerLanguage("json", json);
hljs.registerLanguage("bash", bash);
hljs.registerLanguage("shell", bash);
hljs.registerLanguage("css", css);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("js", javascript);
hljs.registerLanguage("markdown", markdown);
hljs.registerLanguage("md", markdown);
hljs.registerLanguage("powershell", powershell);
hljs.registerLanguage("python", python);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("ts", typescript);
hljs.registerLanguage("html", xml);
hljs.registerLanguage("xml", xml);

const COLLAPSE_AT = 28_000;

export const MarkdownBody = memo(function MarkdownBody({ source, basePath }: { source: string; basePath?: string }) {
  const [expanded, setExpanded] = useState(false);
  const displaySource = useMemo(() => cleanDisplayText(source), [source]);
  const clipped = displaySource.length > COLLAPSE_AT && !expanded;
  const visibleContent = clipped ? `${displaySource.slice(0, COLLAPSE_AT)}\n\n…` : displaySource;
  const content = useMemo(() => linkifyLocalMentions(visibleContent), [visibleContent]);
  return (
    <div className="markdown-body">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        skipHtml
        components={{
          img: ({ src, alt }) => <SafeImage src={src} alt={alt} basePath={basePath} />,
          pre: ({ children }) => <CopyableCodeBlock>{children}</CopyableCodeBlock>,
          a: ({ href, children }) => isLocalReference(href) || href?.startsWith("#local-file=")
            ? <LocalFileLink href={href || ""} basePath={basePath}>{children}</LocalFileLink>
            : <a href={href} target="_blank" rel="noreferrer">{children}</a>,
        }}
      >
        {content}
      </ReactMarkdown>
      {displaySource.length > COLLAPSE_AT && (
        <button className="text-expander" onClick={() => setExpanded((value) => !value)}>
          {expanded ? "Uzun içeriği daralt" : `Tam metni göster · ${Math.round(displaySource.length / 1000)}K karakter`}
        </button>
      )}
    </div>
  );
});

function CopyableCodeBlock({ children }: { children: ReactNode }) {
  const first = Children.toArray(children)[0];
  const className = isValidElement<{ className?: string }>(first) ? first.props.className || "" : "";
  const language = /language-([^\s]+)/.exec(className)?.[1];
  const value = textContent(children).replace(/\n$/, "");
  return (
    <div className="code-block-shell">
      <div className="code-block-toolbar">
        <span>{language || "Kod"}</span>
        <CopyAction value={value} title="Kod bloğunu kopyala" />
      </div>
      <pre>{children}</pre>
    </div>
  );
}

function textContent(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textContent).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return textContent(node.props.children);
  return "";
}

function LocalFileLink({ href, basePath, children }: { href: string; basePath?: string; children: ReactNode }) {
  const [missing, setMissing] = useState(false);
  const raw = href.startsWith("#local-file=") ? decodeURIComponent(href.slice(12)) : href;
  const path = resolveLocalPath(raw, basePath);
  const open = async () => {
    try {
      const revealed = await revealPath(path);
      setMissing(revealed !== path);
    } catch {
      setMissing(true);
    }
    window.setTimeout(() => setMissing(false), 1800);
  };
  return <button className={`inline-file-link ${missing ? "missing" : ""}`} onClick={() => void open()} title={`${path} · klasörde göster`}>{children}</button>;
}

function linkifyLocalMentions(source: string) {
  const matches = findWindowsPaths(source);
  if (!matches.length) return source;
  let output = "";
  let cursor = 0;
  for (const match of matches) {
    output += source.slice(cursor, match.start);
    const label = match.label.replace(/([\\\[\]])/g, "\\$1");
    output += `[${label}](#local-file=${encodeURIComponent(match.path)})`;
    cursor = match.end;
  }
  return output + source.slice(cursor);
}

const FILE_EXTENSIONS = ["docx", "doc", "pdf", "md", "txt", "rtf", "xlsx", "xls", "csv", "pptx", "ppt", "json", "jsonl", "toml", "yaml", "yml", "xml", "html", "css", "tsx", "ts", "jsx", "js", "py", "rs", "png", "jpg", "jpeg", "gif", "webp", "svg", "zip", "7z"];

function findWindowsPaths(source: string) {
  const matches: Array<{ start: number; end: number; path: string; label: string }> = [];
  const drive = /[a-zA-Z]:\\/g;
  let found: RegExpExecArray | null;
  while ((found = drive.exec(source))) {
    const driveStart = found.index;
    const quoted = driveStart >= 2 && source.slice(driveStart - 2, driveStart) === '@"';
    const atPrefixed = !quoted && driveStart >= 1 && source[driveStart - 1] === "@";
    const start = quoted ? driveStart - 2 : atPrefixed ? driveStart - 1 : driveStart;
    let pathEnd = driveStart + 3;
    while (pathEnd < source.length && !/[\r\n"<>|]/.test(source[pathEnd])) pathEnd += 1;
    const candidate = source.slice(driveStart, pathEnd);
    const extension = new RegExp(`\\.(?:${FILE_EXTENSIONS.join("|")})(?=$|[^a-zA-Z0-9])`, "i").exec(candidate);
    if (extension) {
      pathEnd = driveStart + extension.index + extension[0].length;
    } else {
      const wideSpace = /\s{2,}/.exec(candidate);
      if (wideSpace) {
        pathEnd = driveStart + wideSpace.index;
      } else if (/\s/.test(candidate.split("\\").at(-1) || "")) {
        continue;
      }
    }
    let path = source.slice(driveStart, pathEnd).trimEnd().replace(/[.,;:!?]+$/, "");
    pathEnd = driveStart + path.length;
    if (path.length < 4) continue;
    const end = quoted && source[pathEnd] === '"' ? pathEnd + 1 : pathEnd;
    const label = source.slice(start, end);
    matches.push({ start, end, path, label });
    drive.lastIndex = Math.max(end, drive.lastIndex);
  }
  return matches;
}

function SafeImage({ src, alt, basePath }: { src?: string; alt?: string; basePath?: string }) {
  const [failed, setFailed] = useState(false);
  if (!src || failed) {
    return <span className="image-fallback"><ImageOff size={16} /><span>{alt || "Görsel artık bulunamıyor"}</span><small>{src || "Kaynak adresi yok"}</small></span>;
  }
  if (isLocalReference(src)) {
    const path = resolveLocalPath(src, basePath);
    return (
      <button className="image-fallback local" onClick={() => void revealPath(path)} title={path}>
        <FileImage size={16} /><span>{alt || "Yerel görsel"}</span><small>Dosyayı veya bulunduğu klasörü göster</small>
      </button>
    );
  }
  return <img src={src} alt={alt || "Konuşma görseli"} loading="lazy" onError={() => setFailed(true)} />;
}

function isLocalReference(value?: string) {
  if (!value) return false;
  return value.startsWith("file://") || /^[a-zA-Z]:[\\/]/.test(value) || value.startsWith("/") || (!/^[a-z]+:/i.test(value) && !value.startsWith("#"));
}

function resolveLocalPath(value: string, basePath?: string) {
  let decoded = value.replace(/^file:\/\/\/?/i, "");
  try {
    decoded = decodeURIComponent(decoded);
  } catch {
    // Keep the literal reference visible when a provider emitted malformed URI escapes.
  }
  const cleaned = decoded.replace(/\//g, "\\");
  if (/^[a-zA-Z]:\\/.test(cleaned) || cleaned.startsWith("\\\\") || !basePath) return cleaned;
  return `${basePath.replace(/[\\/]+$/, "")}\\${cleaned.replace(/^[\\/]+/, "")}`;
}

export const HighlightedJson = memo(function HighlightedJson({ source }: { source: string }) {
  const formatted = useMemo(() => {
    try {
      return JSON.stringify(JSON.parse(source), null, 2);
    } catch {
      return source;
    }
  }, [source]);
  const highlighted = useMemo(() => hljs.highlight(formatted, { language: "json" }).value, [formatted]);
  return <code className="hljs language-json" dangerouslySetInnerHTML={{ __html: highlighted }} />;
});
