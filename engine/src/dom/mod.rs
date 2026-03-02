/// HTML parser — tokenizer + tree builder (HTML5 subset)

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Document {
    pub root: Node,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    Document,
    Element(Element),
    Text(String),
    Comment(String),
}

#[derive(Debug, Clone)]
pub struct Element {
    pub tag: String,
    pub attrs: HashMap<String, String>,
}

impl Node {
    pub fn new_element(tag: &str) -> Self {
        Self {
            kind: NodeKind::Element(Element {
                tag: tag.to_lowercase(),
                attrs: HashMap::new(),
            }),
            children: Vec::new(),
        }
    }

    pub fn new_text(text: &str) -> Self {
        Self {
            kind: NodeKind::Text(text.into()),
            children: Vec::new(),
        }
    }

    pub fn tag(&self) -> &str {
        match &self.kind {
            NodeKind::Element(e) => &e.tag,
            _ => "",
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        match &self.kind {
            NodeKind::Element(e) => e.attrs.get(name).map(|s| s.as_str()),
            _ => None,
        }
    }

    pub fn set_attr(&mut self, name: &str, value: &str) {
        if let NodeKind::Element(ref mut e) = self.kind {
            e.attrs.insert(name.into(), value.into());
        }
    }

    /// Get all text content recursively
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        match &self.kind {
            NodeKind::Text(t) => out.push_str(t),
            _ => {
                for child in &self.children {
                    child.collect_text(out);
                }
            }
        }
    }

    /// Find elements by tag name
    pub fn find_all(&self, tag: &str) -> Vec<&Node> {
        let mut results = Vec::new();
        self.find_all_recursive(tag, &mut results);
        results
    }

    fn find_all_recursive<'a>(&'a self, tag: &str, results: &mut Vec<&'a Node>) {
        if self.tag() == tag {
            results.push(self);
        }
        for child in &self.children {
            child.find_all_recursive(tag, results);
        }
    }

    /// Find first element by tag
    pub fn find(&self, tag: &str) -> Option<&Node> {
        if self.tag() == tag {
            return Some(self);
        }
        for child in &self.children {
            if let Some(found) = child.find(tag) {
                return Some(found);
            }
        }
        None
    }

    /// Find elements by attribute
    pub fn find_by_attr(&self, attr: &str, value: &str) -> Vec<&Node> {
        let mut results = Vec::new();
        self.find_by_attr_recursive(attr, value, &mut results);
        results
    }

    fn find_by_attr_recursive<'a>(&'a self, attr: &str, value: &str, results: &mut Vec<&'a Node>) {
        if self.attr(attr) == Some(value) {
            results.push(self);
        }
        for child in &self.children {
            child.find_by_attr_recursive(attr, value, results);
        }
    }

    /// Get all links (href attributes from <a> tags)
    pub fn links(&self) -> Vec<&str> {
        self.find_all("a")
            .iter()
            .filter_map(|n| n.attr("href"))
            .collect()
    }
}

// ── HTML Parser ─────────────────────────────────────────────────────────────

const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input",
    "link", "meta", "param", "source", "track", "wbr",
];

const RAW_TEXT_ELEMENTS: &[&str] = &["script", "style", "textarea"];

pub fn parse(html: &str) -> Document {
    let mut parser = HtmlParser::new(html);
    parser.parse();
    let title = parser.find_title();
    Document {
        root: parser.root,
        title,
    }
}

struct HtmlParser {
    chars: Vec<char>,
    pos: usize,
    root: Node,
    stack: Vec<usize>, // indices into a flat node list
}

impl HtmlParser {
    fn new(html: &str) -> Self {
        Self {
            chars: html.chars().collect(),
            pos: 0,
            root: Node {
                kind: NodeKind::Document,
                children: Vec::new(),
            },
            stack: Vec::new(),
        }
    }

    fn peek(&self) -> char {
        self.chars.get(self.pos).copied().unwrap_or('\0')
    }

    fn advance(&mut self) -> char {
        let c = self.peek();
        self.pos += 1;
        c
    }

    fn starts_with(&self, s: &str) -> bool {
        let remaining: String = self.chars[self.pos..].iter().take(s.len()).collect();
        remaining.eq_ignore_ascii_case(s)
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.chars.len() && self.peek().is_whitespace() {
            self.advance();
        }
    }

    fn parse(&mut self) {
        while self.pos < self.chars.len() {
            if self.peek() == '<' {
                if self.starts_with("<!--") {
                    self.parse_comment();
                } else if self.starts_with("<!") || self.starts_with("<?") {
                    // DOCTYPE or processing instruction — skip
                    while self.pos < self.chars.len() && self.peek() != '>' {
                        self.advance();
                    }
                    self.advance(); // >
                } else if self.starts_with("</") {
                    self.parse_close_tag();
                } else {
                    self.parse_open_tag();
                }
            } else {
                self.parse_text();
            }
        }
    }

    fn parse_comment(&mut self) {
        // Skip <!--
        for _ in 0..4 { self.advance(); }
        let mut text = String::new();
        while self.pos < self.chars.len() {
            if self.starts_with("-->") {
                for _ in 0..3 { self.advance(); }
                break;
            }
            text.push(self.advance());
        }
        let comment = Node {
            kind: NodeKind::Comment(text),
            children: Vec::new(),
        };
        self.append_node(comment);
    }

    fn parse_text(&mut self) {
        let mut text = String::new();
        while self.pos < self.chars.len() && self.peek() != '<' {
            let c = self.advance();
            if c == '&' {
                text.push_str(&self.parse_entity());
            } else {
                text.push(c);
            }
        }
        if !text.is_empty() {
            self.append_node(Node::new_text(&text));
        }
    }

    fn parse_entity(&mut self) -> String {
        let mut name = String::new();
        while self.pos < self.chars.len() && self.peek() != ';' && name.len() < 10 {
            name.push(self.advance());
        }
        if self.peek() == ';' { self.advance(); }
        match name.as_str() {
            "amp" => "&".into(),
            "lt" => "<".into(),
            "gt" => ">".into(),
            "quot" => "\"".into(),
            "apos" => "'".into(),
            "nbsp" => "\u{00A0}".into(),
            s if s.starts_with('#') => {
                let code = if s.starts_with("#x") || s.starts_with("#X") {
                    u32::from_str_radix(&s[2..], 16).ok()
                } else {
                    s[1..].parse::<u32>().ok()
                };
                code.and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            }
            _ => format!("&{};", name),
        }
    }

    fn parse_open_tag(&mut self) {
        self.advance(); // <
        let tag = self.read_tag_name();
        if tag.is_empty() { return; }

        let mut element = Node::new_element(&tag);

        // Parse attributes
        loop {
            self.skip_whitespace();
            if self.pos >= self.chars.len() || self.peek() == '>' || self.peek() == '/' { break; }
            let (name, value) = self.parse_attribute();
            if !name.is_empty() {
                element.set_attr(&name, &value);
            }
        }

        // Self-closing or void?
        let self_closing = self.peek() == '/';
        if self_closing { self.advance(); }
        if self.peek() == '>' { self.advance(); }

        let is_void = VOID_ELEMENTS.contains(&tag.as_str()) || self_closing;
        let is_raw = RAW_TEXT_ELEMENTS.contains(&tag.as_str());

        if is_raw && !is_void {
            // Read raw content until closing tag
            let mut raw = String::new();
            let close = format!("</{}", tag);
            while self.pos < self.chars.len() {
                if self.starts_with(&close) {
                    break;
                }
                raw.push(self.advance());
            }
            if !raw.is_empty() {
                element.children.push(Node::new_text(&raw));
            }
            // Skip closing tag
            while self.pos < self.chars.len() && self.peek() != '>' {
                self.advance();
            }
            if self.peek() == '>' { self.advance(); }
            self.append_node(element);
        } else if is_void {
            self.append_node(element);
        } else {
            self.append_node(element);
            self.stack.push(self.current_children_count() - 1);
        }
    }

    fn parse_close_tag(&mut self) {
        self.advance(); // <
        self.advance(); // /
        let tag = self.read_tag_name();
        while self.pos < self.chars.len() && self.peek() != '>' {
            self.advance();
        }
        if self.peek() == '>' { self.advance(); }

        // Pop stack until we find matching open tag
        if !tag.is_empty() {
            while let Some(_) = self.stack.pop() {
                // Simplified: just pop one level
                break;
            }
        }
    }

    fn read_tag_name(&mut self) -> String {
        let mut name = String::new();
        while self.pos < self.chars.len() {
            let c = self.peek();
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ':' {
                name.push(self.advance());
            } else {
                break;
            }
        }
        name.to_lowercase()
    }

    fn parse_attribute(&mut self) -> (String, String) {
        let mut name = String::new();
        while self.pos < self.chars.len() {
            let c = self.peek();
            if c == '=' || c == '>' || c == '/' || c.is_whitespace() { break; }
            name.push(self.advance());
        }
        let name = name.to_lowercase();

        self.skip_whitespace();
        if self.peek() != '=' {
            return (name, String::new());
        }
        self.advance(); // =
        self.skip_whitespace();

        let value = if self.peek() == '"' || self.peek() == '\'' {
            let quote = self.advance();
            let mut val = String::new();
            while self.pos < self.chars.len() && self.peek() != quote {
                let c = self.advance();
                if c == '&' {
                    val.push_str(&self.parse_entity());
                } else {
                    val.push(c);
                }
            }
            if self.peek() == quote { self.advance(); }
            val
        } else {
            let mut val = String::new();
            while self.pos < self.chars.len() && !self.peek().is_whitespace() && self.peek() != '>' {
                val.push(self.advance());
            }
            val
        };

        (name, value)
    }

    fn append_node(&mut self, node: Node) {
        if self.stack.is_empty() {
            self.root.children.push(node);
        } else {
            // Navigate to the current parent using the stack
            let path = self.stack.clone();
            let parent = self.navigate_to(&path);
            parent.children.push(node);
        }
    }

    fn navigate_to(&mut self, path: &[usize]) -> &mut Node {
        let mut current = &mut self.root;
        for &idx in path {
            current = &mut current.children[idx];
        }
        current
    }

    fn current_children_count(&self) -> usize {
        if self.stack.is_empty() {
            self.root.children.len()
        } else {
            let mut current = &self.root;
            for &idx in &self.stack {
                current = &current.children[idx];
            }
            current.children.len()
        }
    }

    fn find_title(&self) -> String {
        if let Some(title) = self.root.find("title") {
            title.text_content()
        } else {
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_html() {
        let doc = parse("<html><head><title>Test</title></head><body><p>Hello</p></body></html>");
        assert_eq!(doc.title, "Test");
        let body = doc.root.find("body").unwrap();
        let p = body.find("p").unwrap();
        assert_eq!(p.text_content(), "Hello");
    }

    #[test]
    fn test_attributes() {
        let doc = parse(r#"<a href="https://example.com" class="link">Click</a>"#);
        let a = doc.root.find("a").unwrap();
        assert_eq!(a.attr("href"), Some("https://example.com"));
        assert_eq!(a.attr("class"), Some("link"));
        assert_eq!(a.text_content(), "Click");
    }

    #[test]
    fn test_void_elements() {
        let doc = parse("<div><br><img src='test.png'><hr></div>");
        let div = doc.root.find("div").unwrap();
        assert!(div.find("br").is_some());
        assert!(div.find("img").is_some());
    }

    #[test]
    fn test_entities() {
        let doc = parse("<p>&amp; &lt; &gt; &quot;</p>");
        let p = doc.root.find("p").unwrap();
        assert_eq!(p.text_content(), "& < > \"");
    }

    #[test]
    fn test_links() {
        let doc = parse(r#"<div><a href="/page1">One</a><a href="/page2">Two</a></div>"#);
        let links = doc.root.links();
        assert_eq!(links, vec!["/page1", "/page2"]);
    }

    #[test]
    fn test_script_raw() {
        let doc = parse("<script>var x = 1 < 2;</script>");
        let script = doc.root.find("script").unwrap();
        assert_eq!(script.text_content(), "var x = 1 < 2;");
    }
}
