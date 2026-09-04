import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";

export function CopyAction({ value, title, label = "Kopyala", className = "" }: { value: string; title: string; label?: string; className?: string }) {
  const [state, setState] = useState<"idle" | "copied" | "error">("idle");
  const resetTimer = useRef<number | null>(null);

  useEffect(() => () => {
    if (resetTimer.current != null) window.clearTimeout(resetTimer.current);
  }, []);

  const copy = async () => {
    const copied = await copyToClipboard(value);
    setState(copied ? "copied" : "error");
    if (resetTimer.current != null) window.clearTimeout(resetTimer.current);
    resetTimer.current = window.setTimeout(() => setState("idle"), 1400);
  };
  const text = state === "copied" ? "Kopyalandı" : state === "error" ? "Kopyalanamadı" : label;

  return (
    <button
      className={`copy-action ${className} ${state}`.trim()}
      onClick={() => void copy()}
      disabled={!value}
      aria-label={state === "copied" ? "Kopyalandı" : title}
      title={title}
    >
      {state === "copied" ? <Check size={12} /> : <Copy size={12} />}
      <span>{text}</span>
    </button>
  );
}

export async function copyToClipboard(value: string) {
  try {
    await navigator.clipboard.writeText(value);
    return true;
  } catch {
    const field = document.createElement("textarea");
    field.value = value;
    field.style.position = "fixed";
    field.style.opacity = "0";
    document.body.appendChild(field);
    field.select();
    const copied = document.execCommand("copy");
    field.remove();
    return copied;
  }
}
