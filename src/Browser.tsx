import { useEffect, useRef, useState } from "react";
// Service worker (sw.js) intercepts all cross-origin requests and proxies them

interface BrowserState {
  url: string;
  title: string;
  loading: boolean;
}

function toUrl(input: string): string {
  input = input.trim();
  if (!input) return "";
  if (/^https?:\/\//i.test(input)) return input;
  if (/^[\w-]+\.[\w.]{2,}/.test(input) && !input.includes(" ")) return `https://${input}`;
  return `https://www.google.com/search?q=${encodeURIComponent(input)}&hl=en`;
}

export function Browser() {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [state, setState] = useState<BrowserState>({
    url: "",
    title: "tensor-browser",
    loading: false,
  });
  const [displayUrl, setDisplayUrl] = useState("");
  const [searchInput, setSearchInput] = useState("");

  // Poll backend for navigation commands from the AI
  useEffect(() => {
    let cancelled = false;
    async function poll() {
      while (!cancelled) {
        try {
          const res = await fetch("/api/state");
          if (res.ok) {
            const data: { url: string } = await res.json();
            if (data.url && data.url !== state.url) {
              navigate(data.url);
            }
          }
        } catch {
          // backend not ready yet
        }
        await new Promise((r) => setTimeout(r, 500));
      }
    }
    poll();
    return () => { cancelled = true; };
  }, [state.url]);

  // Poll for pending eval requests from the AI, execute in iframe, post result back
  useEffect(() => {
    let cancelled = false;
    async function pollEval() {
      while (!cancelled) {
        try {
          const res = await fetch("/api/eval_pending");
          if (res.ok) {
            const data: { pending: string[] } = await res.json();
            for (const id of data.pending) {
              iframeRef.current?.contentWindow?.postMessage({ type: "eval", id }, "*");
            }
          }
        } catch {}
        await new Promise((r) => setTimeout(r, 200));
      }
    }
    pollEval();
    return () => { cancelled = true; };
  }, []);

  // Listen for postMessage events from inside the iframe
  useEffect(() => {
    function onMessage(e: MessageEvent) {
      if (!e.data || typeof e.data !== "object") return;
      if (e.data.type === "navigate" && e.data.url) {
        navigate(e.data.url);
      }
      if (e.data.type === "load") {
        setState((s) => ({
          ...s,
          loading: false,
          title: e.data.title || s.title,
        }));
        setDisplayUrl(e.data.url || "");
      }
      if (e.data.type === "screenshot_result") {
        // forward screenshot data back to backend
        fetch("/api/screenshot_result", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ data: e.data.data }),
        }).catch(() => {});
      }
      if (e.data.type === "eval_result") {
        fetch("/api/eval_result", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ id: e.data.id, result: e.data.result }),
        }).catch(() => {});
      }
    }
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);

  // No srcdoc — iframe navigates to /api/render which gives a real origin

  function navigate(url: string) {
    setState({ url, title: "Loading…", loading: true });
    setDisplayUrl(url);
    setSearchInput(url);
  }

  function onSearchSubmit(e: React.FormEvent) {
    e.preventDefault();
    const url = toUrl(searchInput);
    if (url) navigate(url);
  }

  function onIframeLoad() {
    setState((s) => ({ ...s, loading: false }));
  }

  return (
    <div className="browser">
      <div className="browser-bar">
        <div className="browser-indicator" title="AI-controlled">
          <span className="browser-dot" />
          AI
        </div>
        <form className="browser-search" onSubmit={onSearchSubmit}>
          <input
            className="browser-search-input"
            type="text"
            value={searchInput}
            onChange={(e) => setSearchInput(e.target.value)}
            placeholder="search or enter URL…"
            spellCheck={false}
            autoComplete="off"
          />
        </form>
        {state.loading && <div className="browser-spinner" />}
      </div>
      <div className="browser-viewport">
        {state.url ? (
          <iframe
            ref={iframeRef}
            key={state.url}
            src={`/api/render?url=${encodeURIComponent(state.url)}`}
            title={state.title}
            /* sandbox removed — /api/render is same-origin, security handled server-side */
            onLoad={onIframeLoad}
          />
        ) : (
          <div className="browser-idle">
            <div className="browser-idle-text">waiting for AI…</div>
          </div>
        )}
      </div>
    </div>
  );
}
