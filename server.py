"""tensor-browser — proxy backend."""
from __future__ import annotations

import asyncio
import mimetypes
from urllib.parse import urljoin, urlparse, urlencode, quote
from typing import Optional

import httpx
from bs4 import BeautifulSoup
from fastapi import FastAPI, Query
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import Response, StreamingResponse
from fastapi.staticfiles import StaticFiles
from pathlib import Path
from pydantic import BaseModel

app = FastAPI()

# ── AI-controlled browser state ──────────────────────────────────────────────
_current_url: str = ""
_screenshot_result: Optional[str] = None
_eval_results: dict[str, str] = {}
_eval_events: dict[str, asyncio.Event] = {}
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)

PROXY_BASE = "/api/proxy"
RESOURCE_BASE = "/api/resource"

_STRIP_HEADERS = {
    "x-frame-options",
    "content-security-policy",
    "x-content-type-options",
    "cross-origin-opener-policy",
    "cross-origin-embedder-policy",
    "cross-origin-resource-policy",
}

_CLIENT = httpx.Client(
    follow_redirects=True,
    timeout=15.0,
    headers={
        "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        "Accept-Language": "en-US,en;q=0.5",
    },
)


def _proxy_url(url: str) -> str:
    return f"{PROXY_BASE}?url={quote(url, safe='')}"


def _resource_url(url: str) -> str:
    return f"{RESOURCE_BASE}?url={quote(url, safe='')}"


def _rewrite_html(html: str, base_url: str) -> str:
    soup = BeautifulSoup(html, "html.parser")

    # Inject base-rewriting script
    head = soup.find("head") or soup
    script = soup.new_tag("script")
    script.string = f"""
(function() {{
    // Intercept link clicks — navigate within proxy
    document.addEventListener('click', function(e) {{
        const a = e.target.closest('a');
        if (!a || !a.href) return;
        const href = a.href;
        if (href.startsWith('javascript:') || href.startsWith('#')) return;
        e.preventDefault();
        window.parent.postMessage({{type:'navigate', url: href}}, '*');
    }}, true);
    // Report current URL and title
    window.parent.postMessage({{type:'load', url: window.location.href, title: document.title}}, '*');
    new MutationObserver(function() {{
        window.parent.postMessage({{type:'title', title: document.title}}, '*');
    }}).observe(document.querySelector('title') || document.head, {{childList: true, subtree: true}});
}})();
"""
    head.insert(0, script)

    # Rewrite src attributes
    for tag in soup.find_all(["img", "script", "source", "track", "embed"]):
        for attr in ["src", "data-src"]:
            val = tag.get(attr)
            if val and not val.startswith("data:") and not val.startswith("javascript:"):
                abs_url = urljoin(base_url, val)
                tag[attr] = _resource_url(abs_url)

    # Rewrite href for stylesheets
    for tag in soup.find_all("link"):
        rel = tag.get("rel", [])
        if isinstance(rel, list):
            rel = " ".join(rel)
        if "stylesheet" in rel:
            val = tag.get("href")
            if val:
                abs_url = urljoin(base_url, val)
                tag["href"] = _resource_url(abs_url)
        elif tag.get("href"):
            # Other links (favicon etc.) — resource proxy
            val = tag.get("href")
            if val and not val.startswith("data:"):
                abs_url = urljoin(base_url, val)
                tag["href"] = _resource_url(abs_url)

    # Rewrite form actions
    for form in soup.find_all("form"):
        action = form.get("action")
        if action and not action.startswith("javascript:"):
            form["action"] = _proxy_url(urljoin(base_url, action))

    # Rewrite srcset
    for tag in soup.find_all(attrs={"srcset": True}):
        parts = []
        for part in tag["srcset"].split(","):
            part = part.strip()
            tokens = part.split()
            if tokens:
                abs_url = urljoin(base_url, tokens[0])
                tokens[0] = _resource_url(abs_url)
            parts.append(" ".join(tokens))
        tag["srcset"] = ", ".join(parts)

    return str(soup)


def _rewrite_css(css: str, base_url: str) -> str:
    import re
    def replace_url(m):
        inner = m.group(1).strip("'\"")
        if inner.startswith("data:") or inner.startswith("http"):
            return f"url({_resource_url(inner)})"
        abs_url = urljoin(base_url, inner)
        return f"url({_resource_url(abs_url)})"
    return re.sub(r'url\(([^)]+)\)', replace_url, css)


@app.get("/api/proxy")
def proxy(url: str = Query(...)):
    try:
        resp = _CLIENT.get(url)
    except Exception as e:
        return Response(f"<html><body><pre>Error fetching {url}:\n{e}</pre></body></html>",
                        media_type="text/html", status_code=502)

    headers = {k: v for k, v in resp.headers.items()
               if k.lower() not in _STRIP_HEADERS
               and k.lower() not in {"transfer-encoding", "content-encoding", "content-length"}}
    headers["X-Proxied-URL"] = str(resp.url)

    ct = resp.headers.get("content-type", "text/html").split(";")[0].strip()

    if "html" in ct:
        rewritten = _rewrite_html(resp.text, str(resp.url))
        return Response(rewritten, media_type="text/html; charset=utf-8", headers=headers)

    if "css" in ct:
        rewritten = _rewrite_css(resp.text, str(resp.url))
        return Response(rewritten, media_type="text/css; charset=utf-8", headers=headers)

    return Response(resp.content, media_type=ct, headers=headers)


@app.get("/api/resource")
def resource(url: str = Query(...)):
    try:
        resp = _CLIENT.get(url)
    except Exception as e:
        return Response(status_code=502)

    ct = resp.headers.get("content-type", "application/octet-stream").split(";")[0].strip()
    headers = {k: v for k, v in resp.headers.items()
               if k.lower() not in _STRIP_HEADERS
               and k.lower() not in {"transfer-encoding", "content-encoding", "content-length"}}

    if "css" in ct:
        rewritten = _rewrite_css(resp.text, str(resp.url))
        return Response(rewritten, media_type="text/css", headers=headers)

    return Response(resp.content, media_type=ct, headers=headers)


@app.post("/api/fetch_text")
async def fetch_text(req: dict):
    url = req.get("url", "")
    try:
        resp = _CLIENT.get(url)
        soup = BeautifulSoup(resp.text, "html.parser")
        for tag in soup(["script", "style", "nav", "header", "footer"]):
            tag.decompose()
        text = soup.get_text(separator="\n", strip=True)
        return {"text": text[:32000], "url": str(resp.url), "title": soup.title.string if soup.title else ""}
    except Exception as e:
        return {"error": str(e)}


# ── AI tool endpoints ─────────────────────────────────────────────────────────

class NavigateReq(BaseModel):
    url: str

class EvalReq(BaseModel):
    js: str
    id: str = "default"

@app.post("/api/navigate")
async def api_navigate(req: NavigateReq):
    global _current_url
    _current_url = req.url
    return {"ok": True, "url": req.url}

@app.get("/api/state")
async def api_state():
    return {"url": _current_url}

@app.post("/api/screenshot")
async def api_screenshot():
    # Frontend will capture iframe via Canvas on next poll cycle
    # Returns base64 PNG after frontend posts result back
    global _screenshot_result
    _screenshot_result = None
    # Signal frontend to take screenshot via SSE/poll approach
    # For now, frontend sends result back via /api/screenshot_result
    for _ in range(40):  # wait up to 2s
        await asyncio.sleep(0.05)
        if _screenshot_result is not None:
            data = _screenshot_result
            _screenshot_result = None
            return {"data": data}
    return {"error": "screenshot timeout"}

@app.post("/api/screenshot_result")
async def api_screenshot_result(body: dict):
    global _screenshot_result
    _screenshot_result = body.get("data")
    return {"ok": True}

@app.post("/api/eval")
async def api_eval(req: EvalReq):
    _eval_results.pop(req.id, None)
    ev = asyncio.Event()
    _eval_events[req.id] = ev
    # Frontend polls /api/eval_pending and sends postMessage, then posts result back
    await asyncio.wait_for(ev.wait(), timeout=5.0)
    result = _eval_results.pop(req.id, None)
    _eval_events.pop(req.id, None)
    return {"result": result}

@app.get("/api/eval_pending")
async def api_eval_pending():
    return {"pending": list(_eval_events.keys())}

@app.post("/api/eval_result")
async def api_eval_result(body: dict):
    id_ = body.get("id", "default")
    _eval_results[id_] = body.get("result")
    if id_ in _eval_events:
        _eval_events[id_].set()
    return {"ok": True}


# ── static frontend ───────────────────────────────────────────────────────────
STATIC = Path(__file__).parent / "dist"
if STATIC.exists():
    app.mount("/", StaticFiles(directory=STATIC, html=True), name="static")


if __name__ == "__main__":
    import uvicorn
    uvicorn.run("server:app", host="127.0.0.1", port=41901, reload=True)
