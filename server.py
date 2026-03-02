"""tensor-browser — proxy backend."""
from __future__ import annotations

import asyncio
import ctypes
import json
import mimetypes
from urllib.parse import urljoin, urlparse, urlencode, quote
from typing import Optional

from curl_cffi import requests as curl_requests
from bs4 import BeautifulSoup
from fastapi import FastAPI, Query, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import Response, StreamingResponse
from fastapi.staticfiles import StaticFiles
from pathlib import Path
from pydantic import BaseModel

app = FastAPI()

# ── AI-controlled browser state ──────────────────────────────────────────────
_current_url: str = ""
_last_origin: str = ""  # e.g. "https://www.google.com" — for catch-all routing
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
    "content-security-policy-report-only",
    "x-content-type-options",
    "cross-origin-opener-policy",
    "cross-origin-embedder-policy",
    "cross-origin-resource-policy",
    "permissions-policy",
    "report-to",
    "nel",
}

_SESSION = curl_requests.Session(impersonate="chrome131", verify=False)

# Let curl_cffi handle all Chrome headers via impersonation — don't override
_NAV_HEADERS = {}
_RESOURCE_HEADERS = {}


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
    var RESOURCE = '{RESOURCE_BASE}';
    var BASE = '{base_url}';
    var OUR_ORIGIN = location.origin;

    // Check if URL is ours (don't proxy our own endpoints)
    function isOurs(url) {{
        return url.startsWith(OUR_ORIGIN) || url.startsWith('/api/');
    }}

    // Rewrite external URL to go through proxy
    function proxify(url) {{
        try {{
            // Resolve relative URLs against the original page's base
            var abs = new URL(url, BASE).href;
            // Only proxy external URLs, not our own server
            if ((abs.startsWith('http://') || abs.startsWith('https://')) && !abs.startsWith(OUR_ORIGIN)) {{
                return RESOURCE + '?url=' + encodeURIComponent(abs);
            }}
        }} catch(e) {{}}
        return url;
    }}

    // Intercept fetch
    var _origFetch = window.fetch;
    window.fetch = function(input, init) {{
        try {{
            var url = typeof input === 'string' ? input : input.url;
            if (!isOurs(url)) {{
                var p = proxify(url);
                if (typeof input === 'string') input = p;
                else input = new Request(p, input);
            }}
        }} catch(e) {{}}
        return _origFetch.call(this, input, init);
    }};

    // Intercept XHR
    var _origOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function(method, url) {{
        if (!isOurs(url)) url = proxify(url);
        return _origOpen.apply(this, [method, url].concat(Array.prototype.slice.call(arguments, 2)));
    }};

    // Intercept setAttribute for src/href
    var _origSetAttribute = Element.prototype.setAttribute;
    Element.prototype.setAttribute = function(name, value) {{
        if ((name === 'src' || name === 'href') && typeof value === 'string' && value.match(/^https?:\\/\\//) && !isOurs(value)) {{
            value = proxify(value);
        }}
        return _origSetAttribute.call(this, name, value);
    }};

    // Intercept createElement — catch dynamically created script/img/iframe elements
    var _origCreate = document.createElement.bind(document);
    document.createElement = function(tag) {{
        var el = _origCreate(tag);
        var tagLower = tag.toLowerCase();
        if (tagLower === 'script' || tagLower === 'img' || tagLower === 'iframe' || tagLower === 'link') {{
            // Intercept .src and .href property setters
            var srcDesc = Object.getOwnPropertyDescriptor(HTMLScriptElement.prototype, 'src') ||
                          Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src') ||
                          Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, 'src');
            if (srcDesc && srcDesc.set) {{
                var origSet = srcDesc.set;
                Object.defineProperty(el, 'src', {{
                    get: srcDesc.get ? srcDesc.get.bind(el) : undefined,
                    set: function(v) {{ origSet.call(el, (v && !isOurs(v)) ? proxify(v) : v); }},
                    configurable: true
                }});
            }}
        }}
        return el;
    }};

    // Intercept link clicks — navigate via /api/render
    document.addEventListener('click', function(e) {{
        var a = e.target.closest && e.target.closest('a');
        if (!a) return;
        var href = a.getAttribute('href');
        if (!href || href.startsWith('javascript:') || href.startsWith('#') || href.startsWith('/api/')) return;
        e.preventDefault();
        e.stopPropagation();
        try {{
            var abs = new URL(href, BASE).href;
            if (abs.startsWith('http')) {{
                location.href = '/api/render?url=' + encodeURIComponent(abs);
            }}
        }} catch(ex) {{}}
    }}, true);

    // Intercept form submissions
    document.addEventListener('submit', function(e) {{
        var form = e.target;
        if (!form || form.id === 'tb-form') return;
        try {{
            var action = form.getAttribute('data-original-action') || form.getAttribute('action') || '';
            var abs = action.startsWith('http') ? action : new URL(action, BASE).href;
            if (!abs.startsWith('http')) return;
            e.preventDefault();
            e.stopPropagation();
            var params = new URLSearchParams(new FormData(form));
            var url = form.method && form.method.toLowerCase() === 'post'
                ? abs
                : abs + (abs.includes('?') ? '&' : '?') + params.toString();
            location.href = '/api/render?url=' + encodeURIComponent(url);
        }} catch(ex) {{}}
    }}, true);

    // Intercept window.open
    var _origOpen2 = window.open;
    window.open = function(url) {{
        if (url && !isOurs(url)) {{
            try {{
                var abs = new URL(url, BASE).href;
                location.href = '/api/render?url=' + encodeURIComponent(abs);
            }} catch(e) {{}}
            return null;
        }}
        return _origOpen2.apply(this, arguments);
    }};

    // Intercept location assignments (navigation attempts)
    // Can't override location directly, but we can intercept meta refresh and JS navigation
    var _origAssign = location.assign;
    var _origReplace = location.replace;
    if (_origAssign) location.assign = function(url) {{
        if (url && !isOurs(url) && /^https?:/.test(url)) {{
            return _origAssign.call(location, '/api/render?url=' + encodeURIComponent(url));
        }}
        return _origAssign.call(location, url);
    }};
    if (_origReplace) location.replace = function(url) {{
        if (url && !isOurs(url) && /^https?:/.test(url)) {{
            return _origReplace.call(location, '/api/render?url=' + encodeURIComponent(url));
        }}
        return _origReplace.call(location, url);
    }};
}})();
"""
    head.insert(0, script)

    # No base tag needed — /api/render serves same-origin, relative URLs resolve naturally

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

    # Store original absolute action as data attr — JS interceptor reads it
    for form in soup.find_all("form"):
        action = form.get("action")
        if action and not action.startswith("javascript:"):
            form["data-original-action"] = urljoin(base_url, action)

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

    # Strip Google's sg_trbl "trouble accessing" fallback timer
    for s in soup.find_all("script"):
        if s.string and "sg_trbl" in s.string:
            s.decompose()
    # Strip the hidden trouble message element
    for div in soup.find_all(id=True):
        if div.get("style") == "display:none" and "trouble accessing" in (div.get_text() or ""):
            div.decompose()

    # Rewrite Google's bot challenge script (Script 3):
    # - Let knitsail challenge run normally (computes proof)
    # - Intercept V()/X() which do location.replace() — instead POST the
    #   SG_SS cookie to our server so curl_cffi can use it on the next fetch
    # - Replace location reads (J=window.location) with the real Google URL
    parsed_base = urlparse(base_url)
    base_origin = f"{parsed_base.scheme}://{parsed_base.netloc}"
    for s in soup.find_all("script"):
        if not s.string or len(s.string) < 200:
            continue
        txt = s.string
        # Shim location reads so Google's JS parses query params correctly
        txt = txt.replace(
            "var J=window.location",
            f"var J=new URL('{base_url}')"
        )
        # Replace V() — the navigate-after-challenge function
        # Original: function V(a){W("psrt");sctm&&L();window.prs?window.prs(a).catch(function(){X(a)}):X(a)}
        # New: POST cookie to server, then redirect through our proxy
        txt = txt.replace(
            'function V(a){W("psrt");sctm&&L();window.prs?window.prs(a).catch(function(){X(a)}):X(a)}',
            'function V(a){W("psrt");sctm&&L();__sg_nav(a)}'
        )
        # Replace X() — the final fallback that does location.replace()
        # We just need to intercept any call to location.replace
        # X calls location.replace(b) at the end
        txt = txt.replace(
            'b!==void 0&&a.replace(b)',
            'b!==void 0&&__sg_nav(b)'
        )
        s.string = txt

    # Inject the __sg_nav helper that forwards cookies to our server
    nav_script = soup.new_tag("script")
    nav_script.string = f"""
window.__sg_nav = function(url) {{
    // Grab SG_SS cookie that knitsail just set
    var m = document.cookie.match(/SG_SS=([^;]+)/);
    var cookie = m ? 'SG_SS=' + m[1] : '';
    // Resolve relative URL against original base
    var abs;
    try {{ abs = new URL(url, '{base_url}').href; }} catch(e) {{ abs = url; }}
    // Send cookie to our server so curl_cffi can use it
    var xhr = new XMLHttpRequest();
    xhr.open('POST', '/api/set_google_cookie', false);
    xhr.setRequestHeader('Content-Type', 'application/json');
    xhr.send(JSON.stringify({{cookie: cookie, url: abs}}));
    // Navigate through our proxy
    location.href = '/api/render?url=' + encodeURIComponent(abs);
}};
"""
    head = soup.find("head") or soup
    head.append(nav_script)

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


def _error_page(url: str, message: str) -> Response:
    html = f"""<!DOCTYPE html><html><head>
<style>body{{background:#0d0d0d;color:#888;font-family:monospace;display:flex;align-items:center;justify-content:center;height:100vh;margin:0}}
.box{{text-align:center}}.url{{color:#444;font-size:12px;margin-top:8px;word-break:break-all}}.msg{{color:#c66;margin-top:12px;font-size:13px}}</style>
</head><body><div class="box">
<div style="font-size:32px">⚠</div>
<div class="msg">{message}</div>
<div class="url">{url}</div>
</div></body></html>"""
    return Response(html, media_type="text/html; charset=utf-8", status_code=200)


def _google_results_page(q: str, original_url: str) -> Response:
    """Fetch Google results server-side, render a clean results page with toolbar."""
    try:
        resp = _SESSION.get(
            f"https://www.google.com/search?q={quote(q)}&hl=en&num=10",
            headers=_NAV_HEADERS,
        )
    except Exception as e:
        return _error_page(original_url, str(e)[:120])

    soup = BeautifulSoup(resp.text, "html.parser")
    results = []

    for div in soup.select("div.g, div[data-sokoban-container]"):
        a = div.find("a", href=True)
        if not a or not a["href"].startswith("http"):
            continue
        title_el = div.find("h3")
        title = title_el.get_text() if title_el else a.get_text()
        snippet_el = div.find("div", class_="VwiC3b") or div.find("span", class_="st")
        snippet = snippet_el.get_text() if snippet_el else ""
        if title:
            results.append({"url": a["href"], "title": title, "snippet": snippet})

    if not results:
        for a in soup.find_all("a", href=True):
            href = a["href"]
            if href.startswith("/url?q="):
                real_url = href.split("/url?q=")[1].split("&")[0]
                title = a.get_text()
                if title and len(title) > 5 and real_url.startswith("http"):
                    results.append({"url": real_url, "title": title, "snippet": ""})

    items = ""
    for r in results[:10]:
        esc_url = r["url"].replace("&", "&amp;").replace('"', "&quot;")
        esc_title = r["title"].replace("<", "&lt;")
        esc_snippet = r["snippet"].replace("<", "&lt;")
        items += f'''<div style="margin-bottom:24px">
<a href="/api/render?url={quote(r['url'], safe='')}" style="color:#8ab4f8;font-size:16px;text-decoration:none;display:block">{esc_title}</a>
<div style="color:#555;font-size:12px;margin-top:2px">{esc_url[:80]}</div>
<div style="color:#999;font-size:13px;margin-top:4px;line-height:1.4">{esc_snippet}</div>
</div>'''

    toolbar = f'''<div id="tb-bar" style="position:fixed;top:0;left:0;right:0;height:32px;background:#1a1a1a;display:flex;align-items:center;padding:0 10px;z-index:2147483647;font-family:monospace;font-size:13px;gap:8px;border-bottom:1px solid #333">
<span style="color:#0f0;font-size:10px">●</span>
<span style="color:#f88;font-size:11px">AI</span>
<form id="tb-form" style="flex:1;display:flex" onsubmit="event.preventDefault();var v=document.getElementById('tb-input').value.trim();if(!v)return;var u=v;if(!/^https?:\\/\\//i.test(v)){{if(/^[\\w-]+\\.[\\w.]{{2,}}/.test(v)&&!v.includes(' '))u='https://'+v;else u='https://www.google.com/search?q='+encodeURIComponent(v)+'&hl=en';}}location.href='/api/render?url='+encodeURIComponent(u);">
<input id="tb-input" type="text" value="{original_url}" style="flex:1;background:#111;border:1px solid #333;border-radius:4px;color:#ccc;padding:2px 8px;font-family:monospace;font-size:12px;outline:none" spellcheck="false" autocomplete="off">
</form>
</div>
<script>setInterval(function(){{fetch('/api/state').then(function(r){{return r.json()}}).then(function(d){{if(d.url&&d.url!=='{original_url}')location.href='/api/render?url='+encodeURIComponent(d.url)}}).catch(function(){{}});}},500);</script>'''

    html = f"""<!DOCTYPE html><html><head><title>{q} - Google Search</title>
<style>
body{{background:#0d0d0d;color:#ccc;font-family:-apple-system,BlinkMacSystemFont,sans-serif;padding:48px 20px 20px;margin:0;max-width:700px}}
a:hover{{text-decoration:underline}}
</style>
</head><body>
{toolbar}
<div style="color:#7c7;font-size:18px;margin-bottom:16px;border-bottom:1px solid #222;padding-bottom:8px">{q} — {len(results)} results</div>
{items or '<div style="color:#555;font-size:13px;margin-top:20px">no results found</div>'}
</body></html>"""

    return Response(html, media_type="text/html; charset=utf-8")


def _do_render(url: str) -> Response:
    """Render URL via the Rust engine — returns HTML page with rendered BMP."""
    global _last_origin
    if not url.startswith(("http://", "https://")):
        return _error_page(url, "invalid URL")
    parsed = urlparse(url)
    _last_origin = f"{parsed.scheme}://{parsed.netloc}"

    return _do_engine_page(url, 1280, 800)


@app.post("/api/set_google_cookie")
async def set_google_cookie(body: dict):
    """Receive SG_SS cookie from browser-side challenge, set on curl_cffi session."""
    cookie = body.get("cookie", "")
    url = body.get("url", "https://www.google.com")
    if cookie and "=" in cookie:
        name, _, value = cookie.partition("=")
        _SESSION.cookies.set(name.strip(), value.strip(), domain=".google.com")
    return {"ok": True}


@app.get("/api/render")
async def render(url: str = Query(...)):
    """Fetch, rewrite, and serve proxied HTML directly — no iframe."""
    loop = asyncio.get_event_loop()
    return await loop.run_in_executor(None, _do_render, url)


def _do_proxy(url: str) -> Response:
    if not url.startswith(("http://", "https://")):
        return _error_page(url, "invalid URL")
    try:
        resp = _SESSION.get(url, headers=_NAV_HEADERS)
    except Exception as e:
        return _error_page(url, str(e).split("\n")[0][:120])

    headers = {k: v for k, v in resp.headers.items()
               if k.lower() not in _STRIP_HEADERS
               and k.lower() not in {"transfer-encoding", "content-encoding", "content-length"}}
    headers["X-Proxied-URL"] = str(resp.url)
    ct = resp.headers.get("content-type", "text/html").split(";")[0].strip()

    if "html" in ct:
        return Response(_rewrite_html(resp.text, str(resp.url)), media_type="text/html; charset=utf-8")
    if "css" in ct:
        return Response(_rewrite_css(resp.text, str(resp.url)), media_type="text/css; charset=utf-8", headers=headers)
    return Response(resp.content, media_type=ct, headers=headers)


@app.get("/api/proxy")
async def proxy(url: str = Query(...)):
    return await asyncio.get_event_loop().run_in_executor(None, _do_proxy, url)


@app.get("/api/search")
async def search(q: str = Query(...)):
    """Fetch Google results server-side, parse, return clean HTML."""
    try:
        resp = _SESSION.get(
            f"https://www.google.com/search?q={quote(q)}&hl=en&num=10",
            headers=_NAV_HEADERS,
        )
    except Exception as e:
        return _error_page(q, str(e)[:120])

    soup = BeautifulSoup(resp.text, "html.parser")
    results = []

    # Extract search results from Google's HTML
    for div in soup.select("div.g, div[data-sokoban-container]"):
        a = div.find("a", href=True)
        if not a or not a["href"].startswith("http"):
            continue
        title_el = div.find("h3")
        title = title_el.get_text() if title_el else a.get_text()
        snippet_el = div.find("div", class_="VwiC3b") or div.find("span", class_="st")
        snippet = snippet_el.get_text() if snippet_el else ""
        if title:
            results.append({"url": a["href"], "title": title, "snippet": snippet})

    # If selector-based extraction fails, try a broader approach
    if not results:
        for a in soup.find_all("a", href=True):
            href = a["href"]
            if href.startswith("/url?q="):
                real_url = href.split("/url?q=")[1].split("&")[0]
                title = a.get_text()
                if title and len(title) > 5 and real_url.startswith("http"):
                    results.append({"url": real_url, "title": title, "snippet": ""})

    # Build clean results page
    items = ""
    for r in results[:10]:
        items += f'''<div class="r">
            <a href="{r["url"]}" class="t">{r["title"]}</a>
            <div class="u">{r["url"][:80]}</div>
            <div class="s">{r["snippet"]}</div>
        </div>'''

    html = f"""<!DOCTYPE html><html><head>
<style>
body{{background:#0d0d0d;color:#ccc;font-family:sans-serif;padding:20px;margin:0;max-width:700px}}
.q{{color:#7c7;font-size:18px;margin-bottom:16px;border-bottom:1px solid #222;padding-bottom:8px}}
.r{{margin-bottom:20px}}
.t{{color:#8ab4f8;font-size:16px;text-decoration:none}}
.t:hover{{text-decoration:underline}}
.u{{color:#555;font-size:12px;margin-top:2px}}
.s{{color:#999;font-size:13px;margin-top:4px;line-height:1.4}}
.n{{color:#555;font-size:13px;margin-top:20px}}
</style>
<script>
document.addEventListener('click', function(e) {{
    var a = e.target.closest('a');
    if (!a || !a.href) return;
    e.preventDefault();
    window.parent.postMessage({{type:'navigate', url: a.href}}, '*');
}}, true);
</script>
</head><body>
<div class="q">{q} — {len(results)} results</div>
{items or '<div class="n">no results found</div>'}
</body></html>"""

    return Response(html, media_type="text/html; charset=utf-8")


def _do_resource(url: str, method: str = "GET", body: bytes = b"") -> Response:
    parsed = urlparse(url)
    ref_headers = dict(_RESOURCE_HEADERS)
    ref_headers["Referer"] = f"{parsed.scheme}://{parsed.netloc}/"
    ref_headers["Origin"] = f"{parsed.scheme}://{parsed.netloc}"
    try:
        if method == "POST":
            resp = _SESSION.post(url, data=body, headers=ref_headers)
        else:
            resp = _SESSION.get(url, headers=ref_headers)
    except Exception:
        return Response(status_code=502)

    ct = resp.headers.get("content-type", "application/octet-stream").split(";")[0].strip()
    headers = {k: v for k, v in resp.headers.items()
               if k.lower() not in _STRIP_HEADERS
               and k.lower() not in {"transfer-encoding", "content-encoding", "content-length"}}

    if "css" in ct:
        return Response(_rewrite_css(resp.text, str(resp.url)), media_type="text/css", headers=headers)
    return Response(resp.content, media_type=ct, headers=headers)


@app.api_route("/api/resource", methods=["GET", "POST"])
async def resource(request: Request, url: str = Query(...)):
    body = await request.body() if request.method == "POST" else b""
    return await asyncio.get_event_loop().run_in_executor(
        None, _do_resource, url, request.method, body
    )


@app.post("/api/fetch_text")
async def fetch_text(req: dict):
    url = req.get("url", "")
    try:
        resp = _SESSION.get(url)
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


# ── Engine FFI ────────────────────────────────────────────────────────────────

_ENGINE_LIB_PATH = str(Path(__file__).parent / "engine" / "target" / "release" / "libtensor_engine.dylib")


class _RenderResult(ctypes.Structure):
    _fields_ = [
        ("png_data", ctypes.POINTER(ctypes.c_uint8)),
        ("png_len", ctypes.c_size_t),
        ("pixels", ctypes.POINTER(ctypes.c_uint8)),
        ("pixel_len", ctypes.c_size_t),
        ("width", ctypes.c_uint32),
        ("height", ctypes.c_uint32),
        ("title", ctypes.c_char_p),
        ("error", ctypes.c_char_p),
        ("status", ctypes.c_uint16),
        ("box_count", ctypes.c_uint32),
        ("draw_count", ctypes.c_uint32),
    ]


_engine = None


def _get_engine():
    global _engine
    if _engine is None:
        _engine = ctypes.CDLL(_ENGINE_LIB_PATH)
        _engine.render_url.restype = ctypes.POINTER(_RenderResult)
        _engine.render_url.argtypes = [ctypes.c_char_p, ctypes.c_uint32, ctypes.c_uint32]
        _engine.render_result_free.restype = None
        _engine.render_result_free.argtypes = [ctypes.POINTER(_RenderResult)]
    return _engine


def _do_engine_render(url: str, width: int = 1280, height: int = 800) -> Response:
    """Render URL via Rust engine FFI — returns BMP image."""
    try:
        lib = _get_engine()
    except OSError as e:
        return _error_page(url, f"engine not found: {e} — run: cd engine && cargo build --release")

    result_ptr = lib.render_url(url.encode("utf-8"), width, height)
    if not result_ptr:
        return _error_page(url, "engine returned null")

    r = result_ptr.contents
    if r.error:
        err = r.error.decode("utf-8", errors="replace")
        lib.render_result_free(result_ptr)
        return _error_page(url, err)

    # Copy PNG data before freeing
    png_bytes = bytes(ctypes.cast(r.png_data, ctypes.POINTER(ctypes.c_uint8 * r.png_len)).contents)

    lib.render_result_free(result_ptr)
    return Response(png_bytes, media_type="image/png")


@app.get("/api/engine-render")
async def engine_render(
    url: str = Query(...),
    width: int = Query(1280),
    height: int = Query(800),
):
    """Render a URL using the Rust engine (HTML→CSS→layout→GPU→BMP)."""
    loop = asyncio.get_event_loop()
    return await loop.run_in_executor(None, _do_engine_render, url, width, height)


def _do_engine_page(url: str, width: int = 1280, height: int = 800) -> Response:
    """HTML wrapper around engine-rendered BMP with toolbar."""
    img_url = f"/api/engine-render?url={quote(url, safe='')}&width={width}&height={height}"
    html = f"""<!DOCTYPE html><html><head><title>tensor-engine — {url}</title>
<style>
body{{background:#0d0d0d;margin:0;overflow-x:hidden}}
#tb-bar{{position:fixed;top:0;left:0;right:0;height:32px;background:#1a1a1a;display:flex;align-items:center;padding:0 10px;z-index:2147483647;font-family:monospace;font-size:13px;gap:8px;border-bottom:1px solid #333}}
#tb-input{{flex:1;background:#111;border:1px solid #333;border-radius:4px;color:#ccc;padding:2px 8px;font-family:monospace;font-size:12px;outline:none}}
.render{{margin-top:36px;text-align:center}}
img{{max-width:100%;height:auto}}
</style></head><body>
<div id="tb-bar">
<span style="color:#0f0;font-size:10px">●</span>
<span style="color:#f88;font-size:11px">ENGINE</span>
<form style="flex:1;display:flex" onsubmit="event.preventDefault();var v=document.getElementById('tb-input').value.trim();if(!v)return;var u=v;if(!/^https?:\\/\\//i.test(v)){{if(/^[\\w-]+\\.[\\w.]{{2,}}/.test(v)&&!v.includes(' '))u='https://'+v;else u='https://www.google.com/search?q='+encodeURIComponent(v)+'&hl=en';}}location.href='/api/render?url='+encodeURIComponent(u);">
<input id="tb-input" type="text" value="{url}" spellcheck="false" autocomplete="off">
</form>
</div>
<div class="render"><img src="{img_url}" alt="rendered page"></div>
<script>setInterval(function(){{fetch('/api/state').then(function(r){{return r.json()}}).then(function(d){{if(d.url&&d.url!=='{url}')location.href='/api/render?url='+encodeURIComponent(d.url)}}).catch(function(){{}});}},500);</script>
</body></html>"""
    return Response(html, media_type="text/html; charset=utf-8")


# ── root page ────────────────────────────────────────────────────────────────
@app.get("/")
def root():
    return Response("""<!DOCTYPE html><html><head>
<style>
body{background:#0d0d0d;color:#888;font-family:monospace;margin:0}
#tb-bar{position:fixed;top:0;left:0;right:0;height:32px;background:#1a1a1a;display:flex;align-items:center;padding:0 10px;z-index:2147483647;font-size:13px;gap:8px;border-bottom:1px solid #333}
#tb-input{flex:1;background:#111;border:1px solid #333;border-radius:4px;color:#ccc;padding:2px 8px;font-family:monospace;font-size:12px;outline:none}
.idle{display:flex;align-items:center;justify-content:center;height:100vh;color:#444;font-size:14px}
</style>
<script>
// Poll for AI navigation
setInterval(function(){
    fetch('/api/state').then(function(r){return r.json()}).then(function(d){
        if(d.url && d.url !== window.__lastUrl){
            window.__lastUrl = d.url;
            location.href = '/api/render?url=' + encodeURIComponent(d.url);
        }
    });
}, 500);
</script>
</head><body>
<div id="tb-bar">
<span style="color:#0f0;font-size:10px">●</span>
<span style="color:#f88;font-size:11px">ENGINE</span>
<form style="flex:1;display:flex" onsubmit="event.preventDefault();var v=document.getElementById('tb-input').value.trim();if(!v)return;var u=v;if(!/^https?:\\/\\//i.test(v)){if(/^[\\w-]+\\.[\\w.]{2,}/.test(v)&&!v.includes(' '))u='https://'+v;else u='https://www.google.com/search?q='+encodeURIComponent(v)+'&hl=en';}location.href='/api/render?url='+encodeURIComponent(u);">
<input id="tb-input" type="text" placeholder="enter URL…" spellcheck="false" autocomplete="off">
</form>
</div>
<div class="idle">tensor-engine ready</div>
</body></html>""", media_type="text/html; charset=utf-8")


# ── Catch-all: route unmatched paths through the last-rendered origin ────────
# When Google's JS does `location.href = "/search?q=hello&sei=abc"`, the browser
# navigates to our server at `/search?q=hello&sei=abc`. This route catches it
# and proxies to `https://www.google.com/search?q=hello&sei=abc`.
@app.api_route("/{path:path}", methods=["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"])
async def catchall(request: Request, path: str):
    if not _last_origin:
        return root()
    # Reconstruct the full URL on the original domain
    qs = str(request.query_params)
    target = f"{_last_origin}/{path}"
    if qs:
        target += f"?{qs}"
    # Redirect to /api/render so the page gets fully rewritten
    from fastapi.responses import RedirectResponse
    return RedirectResponse(f"/api/render?url={quote(target, safe='')}", status_code=302)


if __name__ == "__main__":
    import uvicorn
    uvicorn.run("server:app", host="127.0.0.1", port=41901, reload=True)
