pub mod js;
pub mod net;
pub mod dom;
pub mod css;
pub mod layout;
pub mod render;
pub mod gpu;

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

/// Result of a render call — owned by Rust, freed by `render_result_free`.
#[repr(C)]
pub struct RenderResult {
    /// PNG image bytes
    pub png_data: *mut u8,
    pub png_len: usize,
    /// RGBA pixel buffer (width * height * 4)
    pub pixels: *mut u8,
    pub pixel_len: usize,
    pub width: u32,
    pub height: u32,
    /// Page title (null-terminated UTF-8)
    pub title: *mut c_char,
    /// Error message (null if success)
    pub error: *mut c_char,
    /// HTTP status code (0 on error)
    pub status: u16,
    /// Number of layout boxes
    pub box_count: u32,
    /// Number of draw commands
    pub draw_count: u32,
}

/// Fetch URL, parse HTML+CSS, layout, render to BMP. Returns heap-allocated RenderResult.
/// Caller must free with `render_result_free`.
#[unsafe(no_mangle)]
pub extern "C" fn render_url(
    url_ptr: *const c_char,
    viewport_width: u32,
    viewport_height: u32,
) -> *mut RenderResult {
    let url = unsafe {
        if url_ptr.is_null() {
            return error_result("null URL");
        }
        match CStr::from_ptr(url_ptr).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return error_result("invalid UTF-8 in URL"),
        }
    };

    let result = do_render(&url, viewport_width, viewport_height);
    Box::into_raw(Box::new(result))
}

/// Free a RenderResult returned by `render_url`.
#[unsafe(no_mangle)]
pub extern "C" fn render_result_free(ptr: *mut RenderResult) {
    if ptr.is_null() { return; }
    unsafe {
        let r = Box::from_raw(ptr);
        if !r.png_data.is_null() {
            Vec::from_raw_parts(r.png_data, r.png_len, r.png_len);
        }
        if !r.pixels.is_null() {
            Vec::from_raw_parts(r.pixels, r.pixel_len, r.pixel_len);
        }
        if !r.title.is_null() {
            drop(CString::from_raw(r.title));
        }
        if !r.error.is_null() {
            drop(CString::from_raw(r.error));
        }
    }
}

fn error_result(msg: &str) -> *mut RenderResult {
    let error = CString::new(msg).unwrap_or_default();
    Box::into_raw(Box::new(RenderResult {
        png_data: ptr::null_mut(),
        png_len: 0,
        pixels: ptr::null_mut(),
        pixel_len: 0,
        width: 0,
        height: 0,
        title: ptr::null_mut(),
        error: error.into_raw(),
        status: 0,
        box_count: 0,
        draw_count: 0,
    }))
}

fn do_render(url: &str, vw: u32, vh: u32) -> RenderResult {
    // Fetch
    let mut client = net::HttpClient::new();
    let resp = match client.get(url) {
        Ok(r) => r,
        Err(e) => {
            let error = CString::new(format!("HTTP error: {}", e)).unwrap_or_default();
            return RenderResult {
                png_data: ptr::null_mut(), png_len: 0,
                pixels: ptr::null_mut(), pixel_len: 0,
                width: 0, height: 0,
                title: ptr::null_mut(),
                error: error.into_raw(),
                status: 0, box_count: 0, draw_count: 0,
            };
        }
    };

    let status = resp.status;
    let html_text = resp.text();

    // Parse HTML
    let doc = dom::parse(&html_text);
    let title_cstr = CString::new(doc.title.clone()).unwrap_or_default();

    // Parse CSS from <style> tags
    let mut css_text = String::new();
    for style_node in doc.root.find_all("style") {
        css_text.push_str(&style_node.text_content());
        css_text.push('\n');
    }
    let stylesheet = css::parse_stylesheet(&css_text);

    // Layout
    let layout_root = layout::layout(&doc, &stylesheet, vw as f32, vh as f32);
    let mut box_count = 0u32;
    let mut max_y = 0.0_f32;
    layout_root.each(&mut |b| {
        box_count += 1;
        let bottom = b.rect.y + b.rect.height + b.padding.bottom + b.border.bottom + b.margin.bottom;
        if bottom > max_y { max_y = bottom; }
    });

    // Canvas height = max of viewport height and actual content height
    let canvas_h = (max_y.ceil() as u32).max(vh);

    // Software paint (uses fontdue for proper text rendering)
    let canvas = render::paint(&layout_root, vw, canvas_h);
    let draw_cmds = gpu::collect_draw_commands(&layout_root);
    let draw_count = draw_cmds.len() as u32;

    // PNG
    let mut png = canvas.to_png();
    let png_len = png.len();
    let png_ptr = png.as_mut_ptr();
    std::mem::forget(png);

    // Pixels
    let mut pixels = canvas.pixels;
    let pixel_len = pixels.len();
    let pixel_ptr = pixels.as_mut_ptr();
    std::mem::forget(pixels);

    RenderResult {
        png_data: png_ptr,
        png_len,
        pixels: pixel_ptr,
        pixel_len,
        width: canvas.width,
        height: canvas.height,
        title: title_cstr.into_raw(),
        error: ptr::null_mut(),
        status,
        box_count,
        draw_count,
    }
}
