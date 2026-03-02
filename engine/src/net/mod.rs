/// HTTP client — raw TCP sockets + TLS 1.2 (from scratch).
/// Zero dependencies.

pub mod tls;

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use tls::TlsStream;

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub url: String,
}

impl Response {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into()
    }

    pub fn content_type(&self) -> &str {
        self.headers.get("content-type").map(|s| s.as_str()).unwrap_or("text/html")
    }
}

pub struct HttpClient {
    pub cookies: HashMap<String, HashMap<String, String>>, // domain -> name -> value
    pub user_agent: String,
    max_redirects: u32,
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            cookies: HashMap::new(),
            user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".into(),
            max_redirects: 10,
        }
    }

    pub fn get(&mut self, url: &str) -> io::Result<Response> {
        self.request("GET", url, &[], None)
    }

    pub fn post(&mut self, url: &str, body: &[u8], content_type: &str) -> io::Result<Response> {
        let headers = [("Content-Type", content_type)];
        self.request("POST", url, &headers, Some(body))
    }

    fn request(
        &mut self,
        method: &str,
        url: &str,
        extra_headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> io::Result<Response> {
        let mut current_url = url.to_string();
        let mut redirects = 0;

        loop {
            let parsed = parse_url(&current_url)?;
            let resp = self.do_request(method, &parsed, extra_headers, body)?;

            // Handle redirects
            if (resp.status == 301 || resp.status == 302 || resp.status == 303 || resp.status == 307 || resp.status == 308)
                && redirects < self.max_redirects
            {
                if let Some(location) = resp.headers.get("location") {
                    current_url = resolve_url(&current_url, location);
                    redirects += 1;
                    continue;
                }
            }

            return Ok(Response {
                status: resp.status,
                headers: resp.headers,
                body: resp.body,
                url: current_url,
            });
        }
    }

    fn do_request(
        &mut self,
        method: &str,
        url: &ParsedUrl,
        extra_headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> io::Result<Response> {
        let addr_str = format!("{}:{}", url.host, url.port);
        let sock_addr = addr_str.to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "DNS lookup failed"))?;
        let stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(10))?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;

        if url.scheme == "https" {
            let tls_stream = TlsStream::connect(stream, &url.host)?;
            return self.send_and_receive(tls_stream, method, url, extra_headers, body);
        }

        self.send_and_receive(stream, method, url, extra_headers, body)
    }

    fn send_and_receive<S: Read + Write>(
        &mut self,
        mut stream: S,
        method: &str,
        url: &ParsedUrl,
        extra_headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> io::Result<Response> {
        // Build request
        let mut req = format!("{} {} HTTP/1.1\r\n", method, url.path_and_query());
        req.push_str(&format!("Host: {}\r\n", url.host));
        req.push_str(&format!("User-Agent: {}\r\n", self.user_agent));
        req.push_str("Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r\n");
        req.push_str("Accept-Encoding: identity\r\n"); // no compression for simplicity
        req.push_str("Connection: close\r\n");

        // Cookies
        let cookie_str = self.get_cookies(&url.host);
        if !cookie_str.is_empty() {
            req.push_str(&format!("Cookie: {}\r\n", cookie_str));
        }

        for (k, v) in extra_headers {
            req.push_str(&format!("{}: {}\r\n", k, v));
        }

        if let Some(b) = body {
            req.push_str(&format!("Content-Length: {}\r\n", b.len()));
        }

        req.push_str("\r\n");
        stream.write_all(req.as_bytes())?;

        if let Some(b) = body {
            stream.write_all(b)?;
        }

        // Read response
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf)?;

        // Parse response
        let (headers_end, status, headers) = parse_http_response(&buf)?;

        // Store cookies
        for (k, v) in &headers {
            if k == "set-cookie" {
                self.parse_set_cookie(&url.host, v);
            }
        }

        let body = if let Some(te) = headers.get("transfer-encoding") {
            if te.contains("chunked") {
                decode_chunked(&buf[headers_end..])
            } else {
                buf[headers_end..].to_vec()
            }
        } else {
            buf[headers_end..].to_vec()
        };

        Ok(Response {
            status,
            headers,
            body,
            url: String::new(), // filled by caller
        })
    }

    fn get_cookies(&self, host: &str) -> String {
        let mut cookies = Vec::new();
        for (domain, jar) in &self.cookies {
            if host.ends_with(domain) || host == domain.trim_start_matches('.') {
                for (name, value) in jar {
                    cookies.push(format!("{}={}", name, value));
                }
            }
        }
        cookies.join("; ")
    }

    fn parse_set_cookie(&mut self, host: &str, header: &str) {
        let parts: Vec<&str> = header.split(';').collect();
        if let Some(cookie) = parts.first() {
            if let Some((name, value)) = cookie.split_once('=') {
                let domain = parts.iter()
                    .find_map(|p| {
                        let p = p.trim();
                        if p.to_lowercase().starts_with("domain=") {
                            Some(p[7..].to_string())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| host.to_string());

                self.cookies
                    .entry(domain)
                    .or_default()
                    .insert(name.trim().to_string(), value.trim().to_string());
            }
        }
    }
}

// ── URL parsing ─────────────────────────────────────────────────────────────

struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path: String,
    query: String,
}

impl ParsedUrl {
    fn path_and_query(&self) -> String {
        if self.query.is_empty() {
            if self.path.is_empty() { "/".into() } else { self.path.clone() }
        } else {
            format!("{}?{}", if self.path.is_empty() { "/" } else { &self.path }, self.query)
        }
    }
}

fn parse_url(url: &str) -> io::Result<ParsedUrl> {
    let (scheme, rest) = if let Some(idx) = url.find("://") {
        (&url[..idx], &url[idx + 3..])
    } else {
        ("http", url)
    };

    let (host_port, path_query) = if let Some(idx) = rest.find('/') {
        (&rest[..idx], &rest[idx..])
    } else {
        (rest, "/")
    };

    let (host, port) = if let Some(idx) = host_port.rfind(':') {
        let port_str = &host_port[idx + 1..];
        if let Ok(p) = port_str.parse::<u16>() {
            (&host_port[..idx], p)
        } else {
            (host_port, if scheme == "https" { 443 } else { 80 })
        }
    } else {
        (host_port, if scheme == "https" { 443 } else { 80 })
    };

    let (path, query) = if let Some(idx) = path_query.find('?') {
        (&path_query[..idx], &path_query[idx + 1..])
    } else {
        (path_query, "")
    };

    Ok(ParsedUrl {
        scheme: scheme.to_string(),
        host: host.to_string(),
        port,
        path: path.to_string(),
        query: query.to_string(),
    })
}

pub fn resolve_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }
    if relative.starts_with("//") {
        let scheme = if base.starts_with("https") { "https:" } else { "http:" };
        return format!("{}{}", scheme, relative);
    }

    let parsed = match parse_url(base) {
        Ok(p) => p,
        Err(_) => return relative.to_string(),
    };

    if relative.starts_with('/') {
        format!("{}://{}:{}{}", parsed.scheme, parsed.host, parsed.port, relative)
    } else {
        let base_path = if let Some(idx) = parsed.path.rfind('/') {
            &parsed.path[..=idx]
        } else {
            "/"
        };
        format!("{}://{}:{}{}{}", parsed.scheme, parsed.host, parsed.port, base_path, relative)
    }
}

// ── HTTP response parsing ───────────────────────────────────────────────────

fn parse_http_response(buf: &[u8]) -> io::Result<(usize, u16, HashMap<String, String>)> {
    let text = String::from_utf8_lossy(buf);
    let header_end = text.find("\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no header end"))?;

    let header_text = &text[..header_end];
    let lines: Vec<&str> = header_text.split("\r\n").collect();

    // Status line: HTTP/1.1 200 OK
    let status_line = lines.first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no status line"))?;
    let parts: Vec<&str> = status_line.splitn(3, ' ').collect();
    let status = parts.get(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    let mut headers = HashMap::new();
    for line in &lines[1..] {
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(key.trim().to_lowercase(), value.trim().to_string());
        }
    }

    Ok((header_end + 4, status, headers))
}

fn decode_chunked(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut pos = 0;
    let text = String::from_utf8_lossy(data);

    loop {
        // Find chunk size line
        let line_end = match text[pos..].find("\r\n") {
            Some(idx) => pos + idx,
            None => break,
        };
        let size_str = text[pos..line_end].trim();
        let chunk_size = match usize::from_str_radix(size_str, 16) {
            Ok(s) => s,
            Err(_) => break,
        };
        if chunk_size == 0 { break; }

        let chunk_start = line_end + 2;
        let chunk_end = chunk_start + chunk_size;
        if chunk_end > data.len() { break; }

        result.extend_from_slice(&data[chunk_start..chunk_end]);
        pos = chunk_end + 2; // skip \r\n after chunk
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_url() {
        let u = parse_url("http://example.com/path?q=1").unwrap();
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, 80);
        assert_eq!(u.path, "/path");
        assert_eq!(u.query, "q=1");
    }

    #[test]
    fn test_parse_url_https() {
        let u = parse_url("https://www.google.com/search?q=hello").unwrap();
        assert_eq!(u.host, "www.google.com");
        assert_eq!(u.port, 443);
        assert_eq!(u.path, "/search");
    }

    #[test]
    fn test_resolve_url() {
        assert_eq!(
            resolve_url("http://example.com/dir/page.html", "/other"),
            "http://example.com:80/other"
        );
        assert_eq!(
            resolve_url("http://example.com/dir/page.html", "sibling.html"),
            "http://example.com:80/dir/sibling.html"
        );
        assert_eq!(
            resolve_url("http://example.com/", "https://other.com/page"),
            "https://other.com/page"
        );
    }
}
