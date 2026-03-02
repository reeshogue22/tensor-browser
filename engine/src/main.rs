use tensor_engine::{js, net, dom, css, layout, render, gpu};

fn main() {
    println!("=== tensor-engine v0.1.0 ===\n");

    // ── JS Engine ───────────────────────────────────────────────────────
    println!("--- JS Engine ---");
    let mut vm = js::VM::new();

    let tests = [
        ("1 + 2 * 3", "7"),
        ("var x = 10; x + 5", "15"),
        ("var name = 'tensor'; name", "'tensor'"),
        ("function add(a, b) { return a + b; } add(3, 4)", "7"),
        ("var obj = {x: 1, y: 2}; obj.x + obj.y", "3"),
        ("var arr = [1, 2, 3]; arr.length", "3"),
        ("typeof 42", "'number'"),
        ("typeof 'hello'", "'string'"),
        ("!false", "true"),
        ("10 > 5 ? 'yes' : 'no'", "'yes'"),
        ("var i = 0; while (i < 5) { i = i + 1; } i", "5"),
        ("function fib(n) { if (n <= 1) { return n; } return fib(n - 1) + fib(n - 2); } fib(10)", "55"),
    ];

    let mut passed = 0;
    for (code, expected) in tests {
        let result = vm.eval(code);
        let result_str = format!("{:?}", result);
        if result_str == expected {
            println!("  PASS: {} = {}", code.chars().take(50).collect::<String>(), result_str);
            passed += 1;
        } else {
            println!("  FAIL: {} = {} (expected {})", code.chars().take(50).collect::<String>(), result_str, expected);
        }
    }
    println!("  {}/{} tests passed\n", passed, tests.len());

    vm.eval("location.href = 'https://example.com'");
    println!("  location.href intercepted: {:?}\n", vm.last_navigation());

    // ── HTML Parser ─────────────────────────────────────────────────────
    println!("--- HTML Parser ---");
    let html = r#"<!DOCTYPE html>
<html>
<head><title>Tensor Browser Test</title></head>
<body>
    <h1>Hello World</h1>
    <p class="intro">This is a <a href="https://example.com">test page</a>.</p>
    <div id="content">
        <ul>
            <li>Item 1</li>
            <li>Item 2</li>
            <li>Item 3</li>
        </ul>
        <img src="test.png" alt="test image">
    </div>
    <script>var x = 1 < 2 && 3 > 1;</script>
</body>
</html>"#;

    let doc = dom::parse(html);
    println!("  title: {}", doc.title);
    println!("  links: {:?}", doc.root.links());
    println!("  h1: {}", doc.root.find("h1").map(|n| n.text_content()).unwrap_or_default());
    println!("  li count: {}", doc.root.find_all("li").len());
    println!("  img src: {}", doc.root.find("img").and_then(|n| n.attr("src")).unwrap_or("none"));
    println!("  script raw: {}", doc.root.find("script").map(|n| n.text_content()).unwrap_or_default());
    println!("  p.intro: {}", doc.root.find_by_attr("class", "intro").first().map(|n| n.text_content()).unwrap_or_default());
    println!();

    // ── HTTP Client (HTTPS) ─────────────────────────────────────────────
    println!("--- HTTP Client ---");
    let mut client = net::HttpClient::new();
    let target_url = std::env::args().nth(1).unwrap_or_else(|| "https://example.com".into());
    match client.get(&target_url) {
        Ok(resp) => {
            println!("  GET {} → {}", target_url, resp.status);
            println!("  body length: {} bytes", resp.body.len());
            let doc = dom::parse(&resp.text());
            println!("  title: {}", doc.title);
            println!("  links: {:?}", doc.root.links());

            // ── CSS + Layout + Render ───────────────────────────────────
            println!("\n--- Renderer ---");

            // Extract <style> content
            let mut css_text = String::new();
            for style_node in doc.root.find_all("style") {
                css_text.push_str(&style_node.text_content());
                css_text.push('\n');
            }
            println!("  CSS text: {} bytes", css_text.len());
            let stylesheet = css::parse_stylesheet(&css_text);
            println!("  CSS rules: {}", stylesheet.rules.len());

            // Layout
            println!("  starting layout...");
            let layout_root = layout::layout(&doc, &stylesheet, 800.0, 600.0);
            println!("  layout done");
            let mut box_count = 0;
            let mut text_count = 0;
            layout_root.each(&mut |b| {
                box_count += 1;
                match &b.kind {
                    layout::BoxKind::Text(t) => {
                        text_count += 1;
                        println!("    text '{}' at ({:.1}, {:.1}) {:.1}x{:.1} fs={:.1}",
                            t.chars().take(30).collect::<String>(),
                            b.rect.x, b.rect.y, b.rect.width, b.rect.height,
                            b.style.font_size());
                    }
                    layout::BoxKind::Block => {
                        println!("    block at ({:.0}, {:.0}) {}x{:.0}",
                            b.rect.x, b.rect.y, b.rect.width, b.rect.height);
                    }
                    _ => {}
                }
            });
            println!("  layout boxes: {} ({} text)", box_count, text_count);

            // GPU Paint
            println!("\n--- GPU Renderer ---");
            let canvas = gpu::gpu_paint(&layout_root, 800, 600);
            println!("  canvas: {}x{} ({} bytes)", canvas.width, canvas.height, canvas.pixels.len());

            // Save as BMP
            let bmp = canvas.to_bmp();
            std::fs::write("/tmp/tensor-render.bmp", &bmp).ok();
            println!("  rendered to /tmp/tensor-render.bmp");
        }
        Err(e) => println!("  HTTP error: {}", e),
    }

    println!("\ntensor-engine ready.");
}
