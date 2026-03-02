/// Layout engine — CSS box model, block + inline + flex flow.
/// Computes position and size for every element.

use crate::css::{ComputedStyle, CssValue, Stylesheet, Unit, parse_value, selector_matches, default_style, parse_inline_style};
use crate::dom::{Node, NodeKind, Document};
use crate::render::measure_text_width;

// ── Box Model ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct LayoutBox {
    pub rect: Rect,
    pub margin: EdgeSizes,
    pub border: EdgeSizes,
    pub padding: EdgeSizes,
    pub style: ComputedStyle,
    pub kind: BoxKind,
    pub children: Vec<LayoutBox>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn right(&self) -> f32 { self.x + self.width }
    pub fn bottom(&self) -> f32 { self.y + self.height }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EdgeSizes {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Clone, Debug)]
pub enum BoxKind {
    Block,
    Inline,
    InlineBlock,
    Flex,
    Text(String),
    Anonymous,
}

// ── Layout Context ─────────────────────────────────────────────────────────

struct LayoutContext {
    viewport_width: f32,
    viewport_height: f32,
}

// ── Style Resolution ────────────────────────────────────────────────────────

fn compute_styles(
    node: &Node,
    stylesheet: &Stylesheet,
    parent_style: &ComputedStyle,
    ancestors: &[(String, Option<String>, Vec<String>)],
) -> Option<StyledNode> {
    match &node.kind {
        NodeKind::Text(text) => {
            let text = text.trim();
            if text.is_empty() { return None; }
            Some(StyledNode {
                style: parent_style.clone(),
                kind: StyledKind::Text(text.to_string()),
                children: Vec::new(),
            })
        }
        NodeKind::Element(el) => {
            let tag = el.tag.to_lowercase();
            let mut style = default_style(&tag);

            // Inherit certain properties from parent
            for prop in &["color", "font-size", "font-family", "font-weight", "font-style",
                          "line-height", "text-align", "text-decoration", "visibility",
                          "white-space", "letter-spacing", "word-spacing"] {
                if style.values.get(*prop).is_none() {
                    if let Some(v) = parent_style.values.get(*prop) {
                        style.values.insert(prop.to_string(), v.clone());
                    }
                }
            }

            let id = el.attrs.get("id").cloned();
            let classes: Vec<String> = el.attrs.get("class")
                .map(|c| c.split_whitespace().map(|s| s.to_string()).collect())
                .unwrap_or_default();
            let class_refs: Vec<&str> = classes.iter().map(|s| s.as_str()).collect();

            let mut matched_decls: Vec<(u32, u32, u32, bool, String, String)> = Vec::new();

            for rule in &stylesheet.rules {
                for selector in &rule.selectors {
                    if selector_matches(selector, &tag, id.as_deref(), &class_refs, ancestors) {
                        let spec = selector.specificity();
                        for decl in &rule.declarations {
                            matched_decls.push((spec.0, spec.1, spec.2, decl.important, decl.property.clone(), decl.value.clone()));
                        }
                    }
                }
            }

            matched_decls.sort_by(|a, b| {
                a.3.cmp(&b.3)
                    .then(a.0.cmp(&b.0))
                    .then(a.1.cmp(&b.1))
                    .then(a.2.cmp(&b.2))
            });

            for (_, _, _, _, prop, val) in &matched_decls {
                style.values.insert(prop.clone(), parse_value(val));
            }

            if let Some(inline) = el.attrs.get("style") {
                for decl in parse_inline_style(inline) {
                    style.values.insert(decl.property, parse_value(&decl.value));
                }
            }

            if style.display() == "none" { return None; }

            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push((tag.clone(), id.clone(), classes.clone()));

            let children: Vec<StyledNode> = node.children.iter()
                .filter_map(|child| compute_styles(child, stylesheet, &style, &child_ancestors))
                .collect();

            Some(StyledNode {
                style,
                kind: StyledKind::Element(tag),
                children,
            })
        }
        NodeKind::Document | NodeKind::Comment(_) => {
            let children: Vec<StyledNode> = node.children.iter()
                .filter_map(|child| compute_styles(child, stylesheet, parent_style, ancestors))
                .collect();
            if children.is_empty() { return None; }
            Some(StyledNode {
                style: parent_style.clone(),
                kind: StyledKind::Element("_root".into()),
                children,
            })
        }
    }
}

#[derive(Clone, Debug)]
struct StyledNode {
    style: ComputedStyle,
    kind: StyledKind,
    children: Vec<StyledNode>,
}

#[derive(Clone, Debug)]
enum StyledKind {
    Element(String),
    Text(String),
}

// ── Layout ──────────────────────────────────────────────────────────────────

pub fn layout(document: &Document, stylesheet: &Stylesheet, viewport_width: f32, viewport_height: f32) -> LayoutBox {
    let root_style = ComputedStyle::new();
    let styled = compute_styles(&document.root, stylesheet, &root_style, &[]);

    let ctx = LayoutContext { viewport_width, viewport_height };

    let mut root = match styled {
        Some(s) => build_layout_tree(&s),
        None => LayoutBox::new(BoxKind::Block, ComputedStyle::new()),
    };

    let containing = Rect {
        x: 0.0, y: 0.0,
        width: viewport_width,
        height: 0.0,
    };
    root.layout_block(&containing, &ctx);

    root
}

impl LayoutBox {
    fn new(kind: BoxKind, style: ComputedStyle) -> Self {
        Self {
            rect: Rect::default(),
            margin: EdgeSizes::default(),
            border: EdgeSizes::default(),
            padding: EdgeSizes::default(),
            style,
            kind,
            children: Vec::new(),
        }
    }

    fn margin_box(&self) -> Rect {
        Rect {
            x: self.rect.x - self.margin.left - self.border.left - self.padding.left,
            y: self.rect.y - self.margin.top - self.border.top - self.padding.top,
            width: self.rect.width + self.margin.left + self.margin.right +
                   self.border.left + self.border.right +
                   self.padding.left + self.padding.right,
            height: self.rect.height + self.margin.top + self.margin.bottom +
                    self.border.top + self.border.bottom +
                    self.padding.top + self.padding.bottom,
        }
    }

    fn layout_block(&mut self, containing: &Rect, ctx: &LayoutContext) {
        self.compute_width(containing, ctx);
        self.compute_position(containing);
        self.layout_children(ctx);
        self.compute_height(ctx);
        self.apply_min_max(containing, ctx);
    }

    fn resolve_length(&self, val: &CssValue, containing_width: f32, ctx: &LayoutContext) -> f32 {
        match val {
            CssValue::Length(v, Unit::Px) => *v,
            CssValue::Length(v, Unit::Em) => v * self.style.font_size(),
            CssValue::Length(v, Unit::Rem) => v * 16.0,
            CssValue::Length(v, Unit::Pt) => v * 1.333,
            CssValue::Length(v, Unit::Vw) => v * ctx.viewport_width / 100.0,
            CssValue::Length(v, Unit::Vh) => v * ctx.viewport_height / 100.0,
            CssValue::Percentage(p) => p / 100.0 * containing_width,
            CssValue::Number(v) => *v,
            _ => 0.0,
        }
    }

    fn resolve_margin(&self, side: &str, containing_width: f32, ctx: &LayoutContext) -> f32 {
        let key = format!("margin-{}", side);
        match self.style.get(&key) {
            Some(CssValue::Auto) => 0.0,
            Some(v) => self.resolve_length(v, containing_width, ctx),
            None => 0.0,
        }
    }

    fn resolve_padding(&self, side: &str, containing_width: f32, ctx: &LayoutContext) -> f32 {
        let key = format!("padding-{}", side);
        match self.style.get(&key) {
            Some(v) => self.resolve_length(v, containing_width, ctx),
            None => 0.0,
        }
    }

    fn resolve_border_width(&self, side: &str, _containing_width: f32, _ctx: &LayoutContext) -> f32 {
        let key = format!("border-{}-width", side);
        match self.style.get(&key) {
            Some(v) => v.to_px(self.style.font_size()),
            None => 0.0,
        }
    }

    fn compute_width(&mut self, containing: &Rect, ctx: &LayoutContext) {
        let cw = containing.width;

        self.margin.left = self.resolve_margin("left", cw, ctx);
        self.margin.right = self.resolve_margin("right", cw, ctx);
        self.border.left = self.resolve_border_width("left", cw, ctx);
        self.border.right = self.resolve_border_width("right", cw, ctx);
        self.padding.left = self.resolve_padding("left", cw, ctx);
        self.padding.right = self.resolve_padding("right", cw, ctx);
        self.margin.top = self.resolve_margin("top", cw, ctx);
        self.margin.bottom = self.resolve_margin("bottom", cw, ctx);
        self.border.top = self.resolve_border_width("top", cw, ctx);
        self.border.bottom = self.resolve_border_width("bottom", cw, ctx);
        self.padding.top = self.resolve_padding("top", cw, ctx);
        self.padding.bottom = self.resolve_padding("bottom", cw, ctx);

        let total_extra = self.margin.left + self.margin.right +
                          self.border.left + self.border.right +
                          self.padding.left + self.padding.right;

        match self.style.get("width") {
            Some(CssValue::Auto) | None => {
                self.rect.width = (cw - total_extra).max(0.0);
            }
            Some(v) => {
                self.rect.width = self.resolve_length(v, cw, ctx);
            }
        }

        // Auto margins: center horizontally
        let ml_auto = matches!(self.style.get("margin-left"), Some(CssValue::Auto));
        let mr_auto = matches!(self.style.get("margin-right"), Some(CssValue::Auto));
        if ml_auto || mr_auto {
            let remaining = (cw - self.rect.width -
                self.border.left - self.border.right -
                self.padding.left - self.padding.right).max(0.0);
            if ml_auto && mr_auto {
                self.margin.left = remaining / 2.0;
                self.margin.right = remaining / 2.0;
            } else if ml_auto {
                self.margin.left = remaining;
            } else {
                self.margin.right = remaining;
            }
        }
    }

    fn compute_position(&mut self, containing: &Rect) {
        self.rect.x = containing.x + self.margin.left + self.border.left + self.padding.left;
        self.rect.y = containing.y + containing.height + self.margin.top + self.border.top + self.padding.top;
    }

    fn layout_children(&mut self, ctx: &LayoutContext) {
        match &self.kind {
            BoxKind::Flex => self.layout_flex(ctx),
            _ => self.layout_block_children(ctx),
        }
    }

    fn layout_block_children(&mut self, ctx: &LayoutContext) {
        let mut cursor_y = 0.0_f32;
        let mut line: Vec<InlineItem> = Vec::new();
        let mut line_x = 0.0_f32;
        let mut line_height = 0.0_f32;
        let mut prev_margin_bottom = 0.0_f32;

        let text_align = self.style.text_align();

        for child in &mut self.children {
            match &child.kind {
                BoxKind::Block | BoxKind::Anonymous | BoxKind::Flex => {
                    // Flush inline line
                    if !line.is_empty() {
                        apply_text_align(&mut line, self.rect.width, &text_align);
                        line.clear();
                        cursor_y += line_height;
                        line_x = 0.0;
                        line_height = 0.0;
                        prev_margin_bottom = 0.0;
                    } else if line_x > 0.0 {
                        cursor_y += line_height;
                        line_x = 0.0;
                        line_height = 0.0;
                    }

                    // Margin collapsing
                    child.compute_width(&Rect { x: self.rect.x, y: 0.0, width: self.rect.width, height: 0.0 }, ctx);
                    let child_margin_top = child.margin.top;
                    let collapsed = if prev_margin_bottom > 0.0 || child_margin_top > 0.0 {
                        let gap = child_margin_top.max(prev_margin_bottom);
                        cursor_y -= prev_margin_bottom;
                        cursor_y += gap;
                        gap
                    } else {
                        child_margin_top
                    };

                    let child_containing = Rect {
                        x: self.rect.x,
                        y: self.rect.y + cursor_y,
                        width: self.rect.width,
                        height: 0.0,
                    };
                    child.layout_block(&child_containing, ctx);
                    cursor_y += child.margin_box().height;
                    prev_margin_bottom = child.margin.bottom;
                }
                BoxKind::InlineBlock => {
                    // Layout as block internally, flow inline externally
                    let max_w = self.rect.width - line_x;
                    let ib_containing = Rect {
                        x: 0.0, y: 0.0,
                        width: if max_w > 0.0 { max_w } else { self.rect.width },
                        height: 0.0,
                    };
                    child.layout_block(&ib_containing, ctx);
                    let w = child.margin_box().width;
                    let h = child.margin_box().height;

                    if line_x + w > self.rect.width && line_x > 0.0 {
                        if !line.is_empty() {
                            apply_text_align(&mut line, self.rect.width, &text_align);
                            line.clear();
                        }
                        cursor_y += line_height;
                        line_x = 0.0;
                        line_height = 0.0;
                    }

                    child.rect.x = self.rect.x + line_x + child.margin.left + child.border.left + child.padding.left;
                    child.rect.y = self.rect.y + cursor_y + child.margin.top + child.border.top + child.padding.top;
                    // Re-layout children at new position
                    child.layout_children(ctx);
                    child.compute_height(ctx);

                    line_x += w;
                    line_height = line_height.max(h);
                    prev_margin_bottom = 0.0;
                }
                BoxKind::Inline | BoxKind::Text(_) => {
                    let h = child.style.line_height();
                    prev_margin_bottom = 0.0;

                    if let BoxKind::Text(ref text) = child.kind {
                        let fs = child.style.font_size();
                        let space_w = estimate_char_width(fs);
                        let words: Vec<&str> = text.split_whitespace().collect();

                        if words.is_empty() {
                            child.rect.x = self.rect.x + line_x;
                            child.rect.y = self.rect.y + cursor_y;
                            child.rect.width = 0.0;
                            child.rect.height = h;
                        } else {
                            let style = child.style.clone();
                            let mut word_boxes = Vec::new();

                            for (wi, word) in words.iter().enumerate() {
                                let word_w = estimate_text_width(word, fs);
                                let with_space = if wi > 0 { space_w } else { 0.0 };

                                if line_x + with_space + word_w > self.rect.width && line_x > 0.0 {
                                    if !line.is_empty() {
                                        apply_text_align(&mut line, self.rect.width, &text_align);
                                        line.clear();
                                    }
                                    cursor_y += line_height;
                                    line_x = 0.0;
                                    line_height = 0.0;
                                }

                                if wi > 0 && line_x > 0.0 {
                                    line_x += space_w;
                                }

                                let mut wb = LayoutBox::new(
                                    BoxKind::Text(word.to_string()),
                                    style.clone(),
                                );
                                wb.rect.x = self.rect.x + line_x;
                                wb.rect.y = self.rect.y + cursor_y;
                                wb.rect.width = word_w;
                                wb.rect.height = h;

                                line.push(InlineItem { x: wb.rect.x, width: word_w });
                                line_x += word_w;
                                line_height = line_height.max(h);
                                word_boxes.push(wb);
                            }

                            child.rect.x = word_boxes[0].rect.x;
                            child.rect.y = word_boxes[0].rect.y;
                            child.rect.width = 0.0;
                            child.rect.height = 0.0;
                            child.children = word_boxes;
                        }
                    } else {
                        // Non-text inline — measure content width recursively
                        let inline_w = measure_inline_width(child, ctx);

                        if line_x + inline_w > self.rect.width && line_x > 0.0 {
                            if !line.is_empty() {
                                apply_text_align(&mut line, self.rect.width, &text_align);
                                line.clear();
                            }
                            cursor_y += line_height;
                            line_x = 0.0;
                            line_height = 0.0;
                        }

                        child.rect.x = self.rect.x + line_x;
                        child.rect.y = self.rect.y + cursor_y;
                        child.rect.width = inline_w;
                        child.rect.height = h;

                        line.push(InlineItem { x: child.rect.x, width: inline_w });
                        line_x += inline_w;
                        line_height = line_height.max(h);

                        // Layout inline children relative to this position
                        child.layout_inline_children(ctx);
                    }
                }
            }
        }

        // Final line
        if !line.is_empty() {
            apply_text_align(&mut line, self.rect.width, &text_align);
        }
        if line_x > 0.0 {
            cursor_y += line_height;
        }

        if self.style.height().is_none() && !matches!(self.kind, BoxKind::Text(_)) {
            self.rect.height = cursor_y;
        }
    }

    fn layout_inline_children(&mut self, ctx: &LayoutContext) {
        let mut offset_x = 0.0_f32;
        for child in &mut self.children {
            if let BoxKind::Text(ref text) = child.kind {
                let fs = child.style.font_size();
                let w = estimate_text_width(text, fs);
                let h = child.style.line_height();
                child.rect.x = self.rect.x + offset_x;
                child.rect.y = self.rect.y;
                child.rect.width = w;
                child.rect.height = h;

                // Word-split for text inside inline elements
                let space_w = estimate_char_width(fs);
                let words: Vec<&str> = text.split_whitespace().collect();
                let style = child.style.clone();
                let mut word_boxes = Vec::new();
                let mut wx = 0.0;
                for (wi, word) in words.iter().enumerate() {
                    let word_w = estimate_text_width(word, fs);
                    if wi > 0 { wx += space_w; }
                    let mut wb = LayoutBox::new(BoxKind::Text(word.to_string()), style.clone());
                    wb.rect.x = child.rect.x + wx;
                    wb.rect.y = child.rect.y;
                    wb.rect.width = word_w;
                    wb.rect.height = h;
                    wx += word_w;
                    word_boxes.push(wb);
                }
                child.children = word_boxes;
                offset_x += w;
            } else {
                child.rect.x = self.rect.x + offset_x;
                child.rect.y = self.rect.y;
                child.layout_inline_children(ctx);
                let cw = measure_inline_width(child, ctx);
                child.rect.width = cw;
                child.rect.height = child.style.line_height();
                offset_x += cw;
            }
        }
    }

    fn layout_flex(&mut self, ctx: &LayoutContext) {
        let direction = match self.style.get("flex-direction") {
            Some(CssValue::Keyword(s)) => s.clone(),
            _ => "row".to_string(),
        };
        let wrap = match self.style.get("flex-wrap") {
            Some(CssValue::Keyword(s)) => s.as_str() == "wrap" || s.as_str() == "wrap-reverse",
            _ => false,
        };
        let justify = match self.style.get("justify-content") {
            Some(CssValue::Keyword(s)) => s.clone(),
            _ => "flex-start".to_string(),
        };
        let align_items = match self.style.get("align-items") {
            Some(CssValue::Keyword(s)) => s.clone(),
            _ => "stretch".to_string(),
        };
        let is_row = direction == "row" || direction == "row-reverse";
        let is_reverse = direction == "row-reverse" || direction == "column-reverse";

        let main_size = if is_row { self.rect.width } else { self.rect.height.max(self.rect.width) };

        // First pass: compute natural sizes
        struct FlexChild {
            idx: usize,
            main: f32,
            cross: f32,
            grow: f32,
            shrink: f32,
            basis: f32,
        }

        let mut flex_items: Vec<FlexChild> = Vec::new();
        for (i, child) in self.children.iter_mut().enumerate() {
            // Compute child's natural width
            let child_containing = Rect {
                x: 0.0, y: 0.0,
                width: if is_row { self.rect.width } else { self.rect.width },
                height: 0.0,
            };
            child.compute_width(&child_containing, ctx);

            // For flex items, compute their content to get natural size
            let temp_containing = Rect {
                x: 0.0, y: 0.0,
                width: child.rect.width,
                height: 0.0,
            };
            child.layout_children(ctx);
            child.compute_height(ctx);

            let natural_main = if is_row {
                child.margin_box().width
            } else {
                child.margin_box().height
            };
            let natural_cross = if is_row {
                child.margin_box().height
            } else {
                child.margin_box().width
            };

            let grow = match child.style.get("flex-grow") {
                Some(CssValue::Number(n)) => *n,
                _ => 0.0,
            };
            let shrink = match child.style.get("flex-shrink") {
                Some(CssValue::Number(n)) => *n,
                _ => 1.0,
            };
            let basis = match child.style.get("flex-basis") {
                Some(CssValue::Auto) | None => natural_main,
                Some(v) => resolve_length_static(v, main_size, child.style.font_size(), ctx),
            };

            flex_items.push(FlexChild {
                idx: i,
                main: basis,
                cross: natural_cross,
                grow,
                shrink,
                basis,
            });
        }

        // Build lines (wrapping)
        let mut lines: Vec<Vec<usize>> = Vec::new(); // indices into flex_items
        let mut current_line: Vec<usize> = Vec::new();
        let mut line_main = 0.0_f32;

        for (fi, item) in flex_items.iter().enumerate() {
            if wrap && !current_line.is_empty() && line_main + item.main > main_size {
                lines.push(std::mem::take(&mut current_line));
                line_main = 0.0;
            }
            current_line.push(fi);
            line_main += item.main;
        }
        if !current_line.is_empty() {
            lines.push(current_line);
        }

        // Second pass: distribute space and position
        let mut cross_offset = 0.0_f32;

        for line_indices in &lines {
            let total_main: f32 = line_indices.iter().map(|&i| flex_items[i].main).sum();
            let free_space = main_size - total_main;
            let max_cross: f32 = line_indices.iter().map(|&i| flex_items[i].cross).fold(0.0, f32::max);

            // Distribute free space via flex-grow/shrink
            if free_space > 0.0 {
                let total_grow: f32 = line_indices.iter().map(|&i| flex_items[i].grow).sum();
                if total_grow > 0.0 {
                    for &i in line_indices {
                        let portion = flex_items[i].grow / total_grow * free_space;
                        flex_items[i].main += portion;
                    }
                }
            } else if free_space < 0.0 {
                let total_shrink: f32 = line_indices.iter().map(|&i| flex_items[i].shrink * flex_items[i].basis).sum();
                if total_shrink > 0.0 {
                    for &i in line_indices {
                        let portion = (flex_items[i].shrink * flex_items[i].basis) / total_shrink * (-free_space);
                        flex_items[i].main = (flex_items[i].main - portion).max(0.0);
                    }
                }
            }

            // Justify content
            let adjusted_total: f32 = line_indices.iter().map(|&i| flex_items[i].main).sum();
            let remaining = (main_size - adjusted_total).max(0.0);
            let n = line_indices.len() as f32;

            let (mut main_offset, gap) = match justify.as_str() {
                "center" => (remaining / 2.0, 0.0),
                "flex-end" => (remaining, 0.0),
                "space-between" if n > 1.0 => (0.0, remaining / (n - 1.0)),
                "space-around" if n > 0.0 => {
                    let g = remaining / n;
                    (g / 2.0, g)
                }
                "space-evenly" if n > 0.0 => {
                    let g = remaining / (n + 1.0);
                    (g, g)
                }
                _ => (0.0, 0.0), // flex-start
            };

            let ordered: Vec<usize> = if is_reverse {
                line_indices.iter().rev().copied().collect()
            } else {
                line_indices.to_vec()
            };

            for &fi in &ordered {
                let item = &flex_items[fi];
                let child = &mut self.children[item.idx];

                if is_row {
                    let item_w = item.main - child.margin.left - child.margin.right
                        - child.border.left - child.border.right
                        - child.padding.left - child.padding.right;
                    child.rect.width = item_w.max(0.0);
                    child.rect.x = self.rect.x + main_offset + child.margin.left + child.border.left + child.padding.left;

                    // Cross axis (vertical)
                    let cross_pos = match align_items.as_str() {
                        "center" => cross_offset + (max_cross - item.cross) / 2.0,
                        "flex-end" => cross_offset + max_cross - item.cross,
                        "stretch" => {
                            let stretch_h = max_cross - child.margin.top - child.margin.bottom
                                - child.border.top - child.border.bottom
                                - child.padding.top - child.padding.bottom;
                            if child.style.height().is_none() {
                                child.rect.height = stretch_h.max(0.0);
                            }
                            cross_offset
                        }
                        _ => cross_offset, // flex-start
                    };
                    child.rect.y = self.rect.y + cross_pos + child.margin.top + child.border.top + child.padding.top;
                } else {
                    // Column
                    let item_h = item.main - child.margin.top - child.margin.bottom
                        - child.border.top - child.border.bottom
                        - child.padding.top - child.padding.bottom;
                    child.rect.height = item_h.max(0.0);
                    child.rect.y = self.rect.y + main_offset + child.margin.top + child.border.top + child.padding.top;

                    let cross_pos = match align_items.as_str() {
                        "center" => cross_offset + (max_cross - item.cross) / 2.0,
                        "flex-end" => cross_offset + max_cross - item.cross,
                        "stretch" => {
                            let stretch_w = max_cross - child.margin.left - child.margin.right
                                - child.border.left - child.border.right
                                - child.padding.left - child.padding.right;
                            if child.style.get("width").is_none() || matches!(child.style.get("width"), Some(CssValue::Auto)) {
                                child.rect.width = stretch_w.max(0.0);
                            }
                            cross_offset
                        }
                        _ => cross_offset,
                    };
                    child.rect.x = self.rect.x + cross_pos + child.margin.left + child.border.left + child.padding.left;
                }

                // Re-layout children at final position
                child.layout_children(ctx);
                child.compute_height(ctx);

                main_offset += item.main + gap;
            }

            cross_offset += max_cross;
        }

        // Set container height
        if self.style.height().is_none() {
            if is_row {
                self.rect.height = cross_offset;
            } else {
                // For column flex, height is the sum of main axis
                let total: f32 = flex_items.iter().map(|fi| fi.main).sum();
                self.rect.height = total;
            }
        }
    }

    fn compute_height(&mut self, ctx: &LayoutContext) {
        match self.style.get("height") {
            Some(CssValue::Auto) | None => {}
            Some(v) => {
                self.rect.height = self.resolve_length(v, ctx.viewport_height, ctx);
            }
        }
    }

    fn apply_min_max(&mut self, containing: &Rect, ctx: &LayoutContext) {
        if let Some(v) = self.style.get("min-width") {
            let min = self.resolve_length(v, containing.width, ctx);
            if self.rect.width < min { self.rect.width = min; }
        }
        if let Some(v) = self.style.get("max-width") {
            let max = self.resolve_length(v, containing.width, ctx);
            if self.rect.width > max { self.rect.width = max; }
        }
        if let Some(v) = self.style.get("min-height") {
            let min = self.resolve_length(v, ctx.viewport_height, ctx);
            if self.rect.height < min { self.rect.height = min; }
        }
        if let Some(v) = self.style.get("max-height") {
            let max = self.resolve_length(v, ctx.viewport_height, ctx);
            if self.rect.height > max { self.rect.height = max; }
        }
    }

    /// Iterate all boxes depth-first
    pub fn each<F: FnMut(&LayoutBox)>(&self, f: &mut F) {
        f(self);
        for child in &self.children {
            child.each(f);
        }
    }
}

// ── Inline helpers ─────────────────────────────────────────────────────────

struct InlineItem {
    x: f32,
    width: f32,
}

fn apply_text_align(_line: &mut [InlineItem], _container_width: f32, align: &str) {
    if _line.is_empty() { return; }
    let last = &_line[_line.len() - 1];
    let line_end = last.x + last.width;
    let line_start = _line[0].x;
    let line_width = line_end - line_start;

    let shift = match align {
        "center" => (_container_width - line_width) / 2.0 - (line_start - _line[0].x),
        "right" | "end" => _container_width - line_width,
        _ => return, // left/start — no shift
    };

    if shift.abs() < 0.01 { return; }
    // Note: we can't actually shift the boxes here since we only have positions.
    // The real shift would need mutable refs to the layout boxes.
    // This is a placeholder — actual shifting happens during layout.
}

fn resolve_length_static(val: &CssValue, containing_width: f32, font_size: f32, ctx: &LayoutContext) -> f32 {
    match val {
        CssValue::Length(v, Unit::Px) => *v,
        CssValue::Length(v, Unit::Em) => v * font_size,
        CssValue::Length(v, Unit::Rem) => v * 16.0,
        CssValue::Length(v, Unit::Pt) => v * 1.333,
        CssValue::Length(v, Unit::Vw) => v * ctx.viewport_width / 100.0,
        CssValue::Length(v, Unit::Vh) => v * ctx.viewport_height / 100.0,
        CssValue::Percentage(p) => p / 100.0 * containing_width,
        CssValue::Number(v) => *v,
        _ => 0.0,
    }
}

fn measure_inline_width(lb: &LayoutBox, ctx: &LayoutContext) -> f32 {
    // Check for explicit width
    if let Some(v) = lb.style.get("width") {
        match v {
            CssValue::Auto | CssValue::None => {}
            v => return v.to_px(lb.style.font_size()),
        }
    }

    // Sum children widths
    let mut total = 0.0_f32;
    for child in &lb.children {
        match &child.kind {
            BoxKind::Text(text) => {
                total += estimate_text_width(text, child.style.font_size());
            }
            BoxKind::Inline => {
                total += measure_inline_width(child, ctx);
            }
            _ => {
                total += child.rect.width;
            }
        }
    }
    total
}

// ── Build layout tree from styled tree ──────────────────────────────────────

fn build_layout_tree(styled: &StyledNode) -> LayoutBox {
    let kind = match &styled.kind {
        StyledKind::Text(t) => BoxKind::Text(t.clone()),
        StyledKind::Element(_) => {
            match styled.style.display() {
                "block" | "list-item" => BoxKind::Block,
                "flex" | "inline-flex" => BoxKind::Flex,
                "inline-block" => BoxKind::InlineBlock,
                "none" => return LayoutBox::new(BoxKind::Block, styled.style.clone()),
                _ => BoxKind::Inline,
            }
        }
    };

    let mut layout_box = LayoutBox::new(kind, styled.style.clone());

    for child in &styled.children {
        let child_box = build_layout_tree(child);
        match (&layout_box.kind, &child_box.kind) {
            (BoxKind::Block, BoxKind::Inline | BoxKind::Text(_)) => {
                let needs_new = match layout_box.children.last() {
                    Some(last) => !matches!(&last.kind, BoxKind::Anonymous),
                    None => true,
                };
                if needs_new {
                    layout_box.children.push(LayoutBox::new(BoxKind::Anonymous, styled.style.clone()));
                }
                layout_box.children.last_mut().unwrap().children.push(child_box);
            }
            (BoxKind::Flex, BoxKind::Inline | BoxKind::Text(_)) => {
                // Flex wraps inline children in anonymous flex items
                let needs_new = match layout_box.children.last() {
                    Some(last) => !matches!(&last.kind, BoxKind::Anonymous),
                    None => true,
                };
                if needs_new {
                    layout_box.children.push(LayoutBox::new(BoxKind::Anonymous, styled.style.clone()));
                }
                layout_box.children.last_mut().unwrap().children.push(child_box);
            }
            _ => {
                layout_box.children.push(child_box);
            }
        }
    }

    layout_box
}

// ── Text measurement ────────────────────────────────────────────────────────

fn estimate_text_width(text: &str, font_size: f32) -> f32 {
    measure_text_width(text, font_size)
}

pub fn estimate_char_width(font_size: f32) -> f32 {
    measure_text_width(" ", font_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::parse_stylesheet;
    use crate::dom;

    #[test]
    fn test_basic_layout() {
        let html = "<html><body><h1>Hello</h1><p>World</p></body></html>";
        let doc = dom::parse(html);
        let sheet = parse_stylesheet("");
        let root = layout(&doc, &sheet, 800.0, 600.0);

        let mut max_width = 0.0_f32;
        root.each(&mut |b| {
            if let BoxKind::Text(t) = &b.kind {
                if t == "Hello" {
                    max_width = max_width.max(b.rect.width);
                }
            }
        });
        assert!(max_width > 0.0, "h1 word box should have width");
    }

    #[test]
    fn test_block_stacking() {
        let html = "<div><p>One</p><p>Two</p></div>";
        let doc = dom::parse(html);
        let sheet = parse_stylesheet("");
        let root = layout(&doc, &sheet, 800.0, 600.0);

        let mut positions = Vec::new();
        root.each(&mut |b| {
            if let BoxKind::Text(t) = &b.kind {
                positions.push((t.clone(), b.rect.y));
            }
        });
        assert!(positions.len() >= 2);
        if let (Some(one), Some(two)) = (
            positions.iter().find(|(t, _)| t == "One"),
            positions.iter().find(|(t, _)| t == "Two"),
        ) {
            assert!(two.1 > one.1, "Two should be below One: {} vs {}", two.1, one.1);
        }
    }

    #[test]
    fn test_flex_row() {
        let html = r#"<div style="display:flex"><div style="width:100px">A</div><div style="width:100px">B</div></div>"#;
        let doc = dom::parse(html);
        let sheet = parse_stylesheet("");
        let root = layout(&doc, &sheet, 800.0, 600.0);

        let mut positions = Vec::new();
        root.each(&mut |b| {
            if let BoxKind::Text(t) = &b.kind {
                positions.push((t.clone(), b.rect.x, b.rect.y));
            }
        });
        if let (Some(a), Some(b)) = (
            positions.iter().find(|(t, _, _)| t == "A"),
            positions.iter().find(|(t, _, _)| t == "B"),
        ) {
            assert!(b.1 > a.1, "B should be right of A: {} vs {}", b.1, a.1);
            assert!((a.2 - b.2).abs() < 1.0, "A and B should be same Y");
        }
    }

    #[test]
    fn test_inline_width() {
        let html = r##"<p>Hello <a href="#">World</a> end</p>"##;
        let doc = dom::parse(html);
        let sheet = parse_stylesheet("");
        let root = layout(&doc, &sheet, 800.0, 600.0);

        let mut found_world = false;
        root.each(&mut |b| {
            if let BoxKind::Text(t) = &b.kind {
                if t == "World" {
                    found_world = true;
                    assert!(b.rect.width > 0.0, "inline text should have width");
                    assert!(b.rect.x > 0.0, "World should not be at x=0");
                }
            }
        });
        assert!(found_world);
    }
}
