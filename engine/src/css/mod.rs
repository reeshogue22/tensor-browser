/// CSS parser + cascade — from scratch.
/// Parses stylesheets, inline styles, computes final styles per element.

use std::collections::HashMap;

// ── CSS Values ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum CssValue {
    Keyword(String),
    Length(f32, Unit),
    Percentage(f32),
    Color(Color),
    Number(f32),
    Auto,
    None,
    Inherit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    Px,
    Em,
    Rem,
    Percent,
    Vw,
    Vh,
    Pt,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
    pub const TRANSPARENT: Self = Self { r: 0, g: 0, b: 0, a: 0 };

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim_start_matches('#');
        match s.len() {
            3 => {
                let r = u8::from_str_radix(&s[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&s[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&s[2..3], 16).ok()? * 17;
                Some(Self { r, g, b, a: 255 })
            }
            6 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                Some(Self { r, g, b, a: 255 })
            }
            8 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                let a = u8::from_str_radix(&s[6..8], 16).ok()?;
                Some(Self { r, g, b, a })
            }
            _ => None,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "black" => Some(Self::BLACK),
            "white" => Some(Self::WHITE),
            "red" => Some(Self { r: 255, g: 0, b: 0, a: 255 }),
            "green" => Some(Self { r: 0, g: 128, b: 0, a: 255 }),
            "blue" => Some(Self { r: 0, g: 0, b: 255, a: 255 }),
            "yellow" => Some(Self { r: 255, g: 255, b: 0, a: 255 }),
            "cyan" | "aqua" => Some(Self { r: 0, g: 255, b: 255, a: 255 }),
            "magenta" | "fuchsia" => Some(Self { r: 255, g: 0, b: 255, a: 255 }),
            "gray" | "grey" => Some(Self { r: 128, g: 128, b: 128, a: 255 }),
            "silver" => Some(Self { r: 192, g: 192, b: 192, a: 255 }),
            "maroon" => Some(Self { r: 128, g: 0, b: 0, a: 255 }),
            "olive" => Some(Self { r: 128, g: 128, b: 0, a: 255 }),
            "navy" => Some(Self { r: 0, g: 0, b: 128, a: 255 }),
            "teal" => Some(Self { r: 0, g: 128, b: 128, a: 255 }),
            "purple" => Some(Self { r: 128, g: 0, b: 128, a: 255 }),
            "orange" => Some(Self { r: 255, g: 165, b: 0, a: 255 }),
            "pink" => Some(Self { r: 255, g: 192, b: 203, a: 255 }),
            "brown" => Some(Self { r: 165, g: 42, b: 42, a: 255 }),
            "transparent" => Some(Self::TRANSPARENT),
            _ => None,
        }
    }
}

impl CssValue {
    pub fn to_px(&self, parent_font_size: f32) -> f32 {
        match self {
            CssValue::Length(v, Unit::Px) => *v,
            CssValue::Length(v, Unit::Em) => v * parent_font_size,
            CssValue::Length(v, Unit::Rem) => v * 16.0, // root = 16px
            CssValue::Length(v, Unit::Pt) => v * 1.333,
            CssValue::Number(v) => *v,
            CssValue::Percentage(p) => p / 100.0 * parent_font_size,
            _ => 0.0,
        }
    }
}

// ── Selectors ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum Selector {
    Tag(String),
    Class(String),
    Id(String),
    Universal,
    Compound(Vec<Selector>),       // div.class#id — all must match
    Descendant(Box<Selector>, Box<Selector>), // A B
}

impl Selector {
    pub fn specificity(&self) -> (u32, u32, u32) {
        match self {
            Selector::Id(_) => (1, 0, 0),
            Selector::Class(_) => (0, 1, 0),
            Selector::Tag(_) => (0, 0, 1),
            Selector::Universal => (0, 0, 0),
            Selector::Compound(parts) => {
                let mut a = 0; let mut b = 0; let mut c = 0;
                for p in parts {
                    let (pa, pb, pc) = p.specificity();
                    a += pa; b += pb; c += pc;
                }
                (a, b, c)
            }
            Selector::Descendant(ancestor, desc) => {
                let (a1, b1, c1) = ancestor.specificity();
                let (a2, b2, c2) = desc.specificity();
                (a1 + a2, b1 + b2, c1 + c2)
            }
        }
    }
}

// ── Rule ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<Declaration>,
}

#[derive(Clone, Debug)]
pub struct Declaration {
    pub property: String,
    pub value: String,
    pub important: bool,
}

// ── Stylesheet ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

// ── Computed Style ──────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ComputedStyle {
    pub values: HashMap<String, CssValue>,
}

impl ComputedStyle {
    pub fn new() -> Self {
        Self { values: HashMap::new() }
    }

    pub fn get(&self, prop: &str) -> Option<&CssValue> {
        self.values.get(prop)
    }

    pub fn display(&self) -> &str {
        match self.values.get("display") {
            Some(CssValue::Keyword(s)) => s.as_str(),
            _ => "inline",
        }
    }

    pub fn color(&self) -> Color {
        match self.values.get("color") {
            Some(CssValue::Color(c)) => *c,
            _ => Color::BLACK,
        }
    }

    pub fn background_color(&self) -> Color {
        match self.values.get("background-color") {
            Some(CssValue::Color(c)) => *c,
            _ => Color::TRANSPARENT,
        }
    }

    pub fn font_size(&self) -> f32 {
        match self.values.get("font-size") {
            Some(v) => v.to_px(16.0),
            None => 16.0,
        }
    }

    pub fn margin(&self, side: &str) -> f32 {
        let key = format!("margin-{}", side);
        match self.values.get(&key) {
            Some(CssValue::Auto) => 0.0,
            Some(v) => v.to_px(16.0),
            None => 0.0,
        }
    }

    pub fn padding(&self, side: &str) -> f32 {
        let key = format!("padding-{}", side);
        match self.values.get(&key) {
            Some(v) => v.to_px(16.0),
            None => 0.0,
        }
    }

    pub fn border_width(&self, side: &str) -> f32 {
        let key = format!("border-{}-width", side);
        match self.values.get(&key) {
            Some(v) => v.to_px(16.0),
            None => 0.0,
        }
    }

    pub fn width(&self) -> Option<f32> {
        match self.values.get("width") {
            Some(CssValue::Auto) | None => None,
            Some(v) => Some(v.to_px(16.0)),
        }
    }

    pub fn height(&self) -> Option<f32> {
        match self.values.get("height") {
            Some(CssValue::Auto) | None => None,
            Some(v) => Some(v.to_px(16.0)),
        }
    }

    pub fn font_weight(&self) -> u16 {
        match self.values.get("font-weight") {
            Some(CssValue::Keyword(s)) => match s.as_str() {
                "bold" | "bolder" => 700,
                "lighter" => 300,
                _ => 400,
            },
            Some(CssValue::Number(n)) => *n as u16,
            _ => 400,
        }
    }

    pub fn is_bold(&self) -> bool {
        self.font_weight() >= 700
    }

    pub fn text_decoration(&self) -> &str {
        match self.values.get("text-decoration") {
            Some(CssValue::Keyword(s)) => s.as_str(),
            _ => "none",
        }
    }

    pub fn position(&self) -> &str {
        match self.values.get("position") {
            Some(CssValue::Keyword(s)) => s.as_str(),
            _ => "static",
        }
    }

    pub fn overflow(&self) -> &str {
        match self.values.get("overflow") {
            Some(CssValue::Keyword(s)) => s.as_str(),
            _ => "visible",
        }
    }

    pub fn line_height(&self) -> f32 {
        match self.values.get("line-height") {
            Some(CssValue::Number(n)) => n * self.font_size(),
            Some(v) => v.to_px(self.font_size()),
            None => self.font_size() * 1.2,
        }
    }

    pub fn text_align(&self) -> String {
        match self.values.get("text-align") {
            Some(CssValue::Keyword(s)) => s.clone(),
            _ => "left".to_string(),
        }
    }

    pub fn min_width(&self) -> Option<&CssValue> {
        self.values.get("min-width")
    }

    pub fn max_width(&self) -> Option<&CssValue> {
        self.values.get("max-width")
    }

    pub fn min_height(&self) -> Option<&CssValue> {
        self.values.get("min-height")
    }

    pub fn max_height(&self) -> Option<&CssValue> {
        self.values.get("max-height")
    }
}

// ── CSS Parser ──────────────────────────────────────────────────────────────

pub fn parse_stylesheet(input: &str) -> Stylesheet {
    let mut parser = CssParser::new(input);
    Stylesheet { rules: parser.parse_rules() }
}

pub fn parse_inline_style(input: &str) -> Vec<Declaration> {
    let mut parser = CssParser::new(input);
    parser.parse_declarations()
}

pub fn parse_value(input: &str) -> CssValue {
    let s = input.trim();
    if s == "auto" { return CssValue::Auto; }
    if s == "none" { return CssValue::None; }
    if s == "inherit" { return CssValue::Inherit; }

    // Color
    if s.starts_with('#') {
        if let Some(c) = Color::from_hex(s) {
            return CssValue::Color(c);
        }
    }
    if s.starts_with("rgb") {
        if let Some(c) = parse_rgb(s) {
            return CssValue::Color(c);
        }
    }
    if let Some(c) = Color::from_name(s) {
        return CssValue::Color(c);
    }

    // Length
    if let Some(v) = parse_length(s) {
        return v;
    }

    // Number
    if let Ok(n) = s.parse::<f32>() {
        return CssValue::Number(n);
    }

    CssValue::Keyword(s.to_lowercase())
}

fn parse_length(s: &str) -> Option<CssValue> {
    let units = [("px", Unit::Px), ("em", Unit::Em), ("rem", Unit::Rem),
                 ("vw", Unit::Vw), ("vh", Unit::Vh), ("pt", Unit::Pt), ("%", Unit::Percent)];
    for (suffix, unit) in &units {
        if s.ends_with(suffix) {
            let num_str = &s[..s.len() - suffix.len()];
            if let Ok(n) = num_str.parse::<f32>() {
                if *unit == Unit::Percent {
                    return Some(CssValue::Percentage(n));
                }
                return Some(CssValue::Length(n, *unit));
            }
        }
    }
    // bare 0
    if s == "0" {
        return Some(CssValue::Length(0.0, Unit::Px));
    }
    None
}

fn parse_rgb(s: &str) -> Option<Color> {
    let inner = s.trim_start_matches("rgba(")
        .trim_start_matches("rgb(")
        .trim_end_matches(')');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() >= 3 {
        let r = parts[0].trim().parse::<u8>().ok()?;
        let g = parts[1].trim().parse::<u8>().ok()?;
        let b = parts[2].trim().parse::<u8>().ok()?;
        let a = if parts.len() >= 4 {
            (parts[3].trim().parse::<f32>().ok()? * 255.0) as u8
        } else {
            255
        };
        Some(Color { r, g, b, a })
    } else {
        None
    }
}

struct CssParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> CssParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn remaining(&self) -> &str {
        &self.input[self.pos..]
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            self.skip_whitespace();
            if self.remaining().starts_with("/*") {
                if let Some(end) = self.remaining().find("*/") {
                    self.pos += end + 2;
                } else {
                    self.pos = self.input.len();
                }
            } else {
                break;
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.input.as_bytes().get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn parse_rules(&mut self) -> Vec<Rule> {
        let mut rules = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.pos >= self.input.len() { break; }
            let start_pos = self.pos;
            // Skip @rules (media queries etc)
            if self.peek() == Some(b'@') {
                self.skip_at_rule();
                continue;
            }
            if let Some(rule) = self.parse_rule() {
                rules.push(rule);
            }
            // Safety: if we didn't advance, skip one char to avoid infinite loop
            if self.pos == start_pos {
                self.advance();
            }
        }
        rules
    }

    fn skip_at_rule(&mut self) {
        let mut depth = 0;
        while self.pos < self.input.len() {
            match self.advance() {
                Some(b'{') => depth += 1,
                Some(b'}') => {
                    depth -= 1;
                    if depth <= 0 { return; }
                }
                Some(b';') if depth == 0 => return,
                None => return,
                _ => {}
            }
        }
    }

    fn parse_rule(&mut self) -> Option<Rule> {
        let selectors = self.parse_selectors()?;
        self.skip_whitespace_and_comments();
        if self.advance() != Some(b'{') { return None; }
        let declarations = self.parse_declarations();
        self.skip_whitespace_and_comments();
        if self.peek() == Some(b'}') { self.advance(); }
        Some(Rule { selectors, declarations })
    }

    fn parse_selectors(&mut self) -> Option<Vec<Selector>> {
        let mut selectors = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.pos >= self.input.len() || self.peek() == Some(b'{') { break; }
            let sel = self.parse_selector()?;
            selectors.push(sel);
            self.skip_whitespace_and_comments();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        if selectors.is_empty() { None } else { Some(selectors) }
    }

    fn parse_selector(&mut self) -> Option<Selector> {
        let mut parts = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.pos >= self.input.len() { break; }
            let b = self.peek()?;
            if b == b'{' || b == b',' { break; }

            let simple = self.parse_simple_selector()?;
            parts.push(simple);
        }
        match parts.len() {
            0 => None,
            1 => Some(parts.remove(0)),
            _ => {
                // Build descendant chain
                let mut iter = parts.into_iter();
                let mut result = iter.next().unwrap();
                for next in iter {
                    result = Selector::Descendant(Box::new(result), Box::new(next));
                }
                Some(result)
            }
        }
    }

    fn parse_simple_selector(&mut self) -> Option<Selector> {
        let mut parts = Vec::new();
        loop {
            match self.peek() {
                Some(b'#') => {
                    self.advance();
                    let name = self.parse_identifier();
                    parts.push(Selector::Id(name));
                }
                Some(b'.') => {
                    self.advance();
                    let name = self.parse_identifier();
                    parts.push(Selector::Class(name));
                }
                Some(b'*') => {
                    self.advance();
                    parts.push(Selector::Universal);
                }
                Some(b':') => {
                    // Pseudo-class/element — skip it (treat as class-level specificity)
                    self.advance();
                    if self.peek() == Some(b':') { self.advance(); } // ::pseudo-element
                    let name = self.parse_identifier();
                    // Skip function args like :nth-child(2n+1)
                    if self.peek() == Some(b'(') {
                        self.advance();
                        let mut depth = 1;
                        while self.pos < self.input.len() && depth > 0 {
                            match self.advance() {
                                Some(b'(') => depth += 1,
                                Some(b')') => depth -= 1,
                                _ => {}
                            }
                        }
                    }
                    if !name.is_empty() {
                        parts.push(Selector::Class(format!(":{}", name)));
                    }
                }
                Some(b) if b.is_ascii_alphabetic() || b == b'-' || b == b'_' => {
                    let name = self.parse_identifier();
                    parts.push(Selector::Tag(name.to_lowercase()));
                }
                _ => break,
            }
        }
        match parts.len() {
            0 => None,
            1 => Some(parts.remove(0)),
            _ => Some(Selector::Compound(parts)),
        }
    }

    fn parse_identifier(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.input.len() {
            let b = self.input.as_bytes()[self.pos];
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.input[start..self.pos].to_string()
    }

    fn parse_declarations(&mut self) -> Vec<Declaration> {
        let mut decls = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.pos >= self.input.len() || self.peek() == Some(b'}') { break; }
            if let Some(decl) = self.parse_declaration() {
                // Expand shorthands
                expand_shorthand(&decl, &mut decls);
            }
        }
        decls
    }

    fn parse_declaration(&mut self) -> Option<Declaration> {
        let property = self.parse_identifier().to_lowercase();
        if property.is_empty() {
            self.advance(); // skip bad char
            return None;
        }
        self.skip_whitespace();
        if self.advance() != Some(b':') { return None; }
        self.skip_whitespace();

        let start = self.pos;
        let mut important = false;
        while self.pos < self.input.len() {
            let b = self.input.as_bytes()[self.pos];
            if b == b';' || b == b'}' { break; }
            self.pos += 1;
        }
        let mut value = self.input[start..self.pos].trim().to_string();
        if value.ends_with("!important") {
            important = true;
            value = value.trim_end_matches("!important").trim().to_string();
        }

        if self.peek() == Some(b';') { self.advance(); }

        Some(Declaration { property, value, important })
    }
}

// ── Shorthand expansion ────────────────────────────────────────────────────

fn expand_shorthand(decl: &Declaration, out: &mut Vec<Declaration>) {
    match decl.property.as_str() {
        "margin" | "padding" => {
            let parts: Vec<&str> = decl.value.split_whitespace().collect();
            let (top, right, bottom, left) = match parts.len() {
                1 => (parts[0], parts[0], parts[0], parts[0]),
                2 => (parts[0], parts[1], parts[0], parts[1]),
                3 => (parts[0], parts[1], parts[2], parts[1]),
                4 => (parts[0], parts[1], parts[2], parts[3]),
                _ => return,
            };
            let prefix = &decl.property;
            for (side, val) in [("top", top), ("right", right), ("bottom", bottom), ("left", left)] {
                out.push(Declaration {
                    property: format!("{}-{}", prefix, side),
                    value: val.to_string(),
                    important: decl.important,
                });
            }
        }
        "border" => {
            // border: 1px solid black
            let parts: Vec<&str> = decl.value.split_whitespace().collect();
            for side in &["top", "right", "bottom", "left"] {
                if let Some(width) = parts.first() {
                    out.push(Declaration {
                        property: format!("border-{}-width", side),
                        value: width.to_string(),
                        important: decl.important,
                    });
                }
                if let Some(style) = parts.get(1) {
                    out.push(Declaration {
                        property: format!("border-{}-style", side),
                        value: style.to_string(),
                        important: decl.important,
                    });
                }
                if let Some(color) = parts.get(2) {
                    out.push(Declaration {
                        property: format!("border-{}-color", side),
                        value: color.to_string(),
                        important: decl.important,
                    });
                }
            }
        }
        "background" => {
            // Simple: just treat as background-color
            out.push(Declaration {
                property: "background-color".into(),
                value: decl.value.clone(),
                important: decl.important,
            });
        }
        "font" => {
            // Skip complex font shorthand, just pass through
            out.push(decl.clone());
        }
        _ => {
            out.push(decl.clone());
        }
    }
}

// ── Selector matching ───────────────────────────────────────────────────────

pub fn selector_matches(selector: &Selector, tag: &str, id: Option<&str>, classes: &[&str], ancestors: &[(String, Option<String>, Vec<String>)]) -> bool {
    match selector {
        Selector::Tag(t) => t == tag,
        Selector::Class(c) => classes.contains(&c.as_str()),
        Selector::Id(i) => id == Some(i.as_str()),
        Selector::Universal => true,
        Selector::Compound(parts) => parts.iter().all(|p| selector_matches(p, tag, id, classes, ancestors)),
        Selector::Descendant(ancestor_sel, desc_sel) => {
            if !selector_matches(desc_sel, tag, id, classes, ancestors) {
                return false;
            }
            ancestors.iter().any(|(atag, aid, aclasses)| {
                let aclass_refs: Vec<&str> = aclasses.iter().map(|s| s.as_str()).collect();
                selector_matches(ancestor_sel, atag, aid.as_deref(), &aclass_refs, &[])
            })
        }
    }
}

// ── Default styles ──────────────────────────────────────────────────────────

pub fn default_style(tag: &str) -> ComputedStyle {
    let mut style = ComputedStyle::new();
    match tag {
        "html" | "body" => {
            style.values.insert("display".into(), CssValue::Keyword("block".into()));
        }
        "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        | "ul" | "ol" | "li" | "header" | "footer" | "main" | "section"
        | "article" | "nav" | "aside" | "blockquote" | "pre" | "form"
        | "table" | "hr" | "figure" | "figcaption" | "details" | "summary" => {
            style.values.insert("display".into(), CssValue::Keyword("block".into()));
        }
        "span" | "a" | "strong" | "b" | "em" | "i" | "u" | "s"
        | "code" | "small" | "big" | "sub" | "sup" | "label"
        | "abbr" | "cite" | "q" | "mark" | "time" => {
            style.values.insert("display".into(), CssValue::Keyword("inline".into()));
        }
        "script" | "style" | "head" | "meta" | "link" | "title" => {
            style.values.insert("display".into(), CssValue::Keyword("none".into()));
        }
        "img" | "input" | "button" | "select" | "textarea" => {
            style.values.insert("display".into(), CssValue::Keyword("inline-block".into()));
        }
        _ => {
            style.values.insert("display".into(), CssValue::Keyword("inline".into()));
        }
    }
    // Tag-specific defaults
    match tag {
        "h1" => {
            style.values.insert("font-size".into(), CssValue::Length(32.0, Unit::Px));
            style.values.insert("font-weight".into(), CssValue::Keyword("bold".into()));
            style.values.insert("margin-top".into(), CssValue::Length(21.44, Unit::Px));
            style.values.insert("margin-bottom".into(), CssValue::Length(21.44, Unit::Px));
        }
        "h2" => {
            style.values.insert("font-size".into(), CssValue::Length(24.0, Unit::Px));
            style.values.insert("font-weight".into(), CssValue::Keyword("bold".into()));
            style.values.insert("margin-top".into(), CssValue::Length(19.92, Unit::Px));
            style.values.insert("margin-bottom".into(), CssValue::Length(19.92, Unit::Px));
        }
        "h3" => {
            style.values.insert("font-size".into(), CssValue::Length(18.72, Unit::Px));
            style.values.insert("font-weight".into(), CssValue::Keyword("bold".into()));
            style.values.insert("margin-top".into(), CssValue::Length(18.72, Unit::Px));
            style.values.insert("margin-bottom".into(), CssValue::Length(18.72, Unit::Px));
        }
        "p" => {
            style.values.insert("margin-top".into(), CssValue::Length(16.0, Unit::Px));
            style.values.insert("margin-bottom".into(), CssValue::Length(16.0, Unit::Px));
        }
        "a" => {
            style.values.insert("color".into(), CssValue::Color(Color { r: 0, g: 0, b: 238, a: 255 }));
            style.values.insert("text-decoration".into(), CssValue::Keyword("underline".into()));
        }
        "strong" | "b" => {
            style.values.insert("font-weight".into(), CssValue::Keyword("bold".into()));
        }
        "em" | "i" => {
            style.values.insert("font-style".into(), CssValue::Keyword("italic".into()));
        }
        "ul" | "ol" => {
            style.values.insert("margin-top".into(), CssValue::Length(16.0, Unit::Px));
            style.values.insert("margin-bottom".into(), CssValue::Length(16.0, Unit::Px));
            style.values.insert("padding-left".into(), CssValue::Length(40.0, Unit::Px));
        }
        "li" => {
            style.values.insert("display".into(), CssValue::Keyword("list-item".into()));
        }
        "body" => {
            style.values.insert("margin-top".into(), CssValue::Length(8.0, Unit::Px));
            style.values.insert("margin-right".into(), CssValue::Length(8.0, Unit::Px));
            style.values.insert("margin-bottom".into(), CssValue::Length(8.0, Unit::Px));
            style.values.insert("margin-left".into(), CssValue::Length(8.0, Unit::Px));
        }
        "pre" | "code" => {
            style.values.insert("font-family".into(), CssValue::Keyword("monospace".into()));
        }
        "hr" => {
            style.values.insert("border-top-width".into(), CssValue::Length(1.0, Unit::Px));
            style.values.insert("margin-top".into(), CssValue::Length(8.0, Unit::Px));
            style.values.insert("margin-bottom".into(), CssValue::Length(8.0, Unit::Px));
        }
        _ => {}
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_color() {
        assert_eq!(Color::from_hex("#ff0000"), Some(Color { r: 255, g: 0, b: 0, a: 255 }));
        assert_eq!(Color::from_hex("#f00"), Some(Color { r: 255, g: 0, b: 0, a: 255 }));
        assert_eq!(Color::from_name("blue"), Some(Color { r: 0, g: 0, b: 255, a: 255 }));
    }

    #[test]
    fn test_parse_stylesheet() {
        let css = "h1 { color: red; font-size: 24px; } .intro { margin: 10px 20px; }";
        let sheet = parse_stylesheet(css);
        assert_eq!(sheet.rules.len(), 2);
        assert_eq!(sheet.rules[0].declarations.len(), 2);
        // margin shorthand expands to 4 declarations
        assert_eq!(sheet.rules[1].declarations.len(), 4);
    }

    #[test]
    fn test_parse_value() {
        assert_eq!(parse_value("16px"), CssValue::Length(16.0, Unit::Px));
        assert_eq!(parse_value("auto"), CssValue::Auto);
        assert_eq!(parse_value("50%"), CssValue::Percentage(50.0));
        assert_eq!(parse_value("red"), CssValue::Color(Color { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn test_selector_specificity() {
        assert_eq!(Selector::Tag("div".into()).specificity(), (0, 0, 1));
        assert_eq!(Selector::Class("foo".into()).specificity(), (0, 1, 0));
        assert_eq!(Selector::Id("bar".into()).specificity(), (1, 0, 0));
    }
}
