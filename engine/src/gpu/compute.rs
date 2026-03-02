/// Compute kernel — parallel draw command processor.
/// Runs on all platforms using the GPU-allocated memory.
/// Architecture mirrors GPU compute: one "thread" per pixel,
/// each thread iterates draw commands and composites.
/// Uses OS threads + tiles for parallelism.

use super::*;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

// ── Kernel descriptor ──────────────────────────────────────────────────────

pub struct Kernel {
    pub width: u32,
    pub height: u32,
    pub cmd_count: u32,
}

// ── Software device (fallback) ─────────────────────────────────────────────

pub struct SoftDevice {
    threads: usize,
    allocations: std::sync::Mutex<Vec<SoftAlloc>>,
    next_handle: AtomicUsize,
}

struct SoftAlloc {
    handle: u64,
    data: Vec<u8>,
    ptr: *mut u8,
}

unsafe impl Send for SoftAlloc {}
unsafe impl Sync for SoftAlloc {}

impl SoftDevice {
    pub fn new() -> Self {
        // Detect CPU core count via platform-specific methods
        let threads = detect_cpu_count();
        SoftDevice {
            threads,
            allocations: std::sync::Mutex::new(Vec::new()),
            next_handle: AtomicUsize::new(1),
        }
    }

    pub fn thread_count(&self) -> usize { self.threads }
}

impl GpuDevice for SoftDevice {
    fn info(&self) -> &GpuInfo {
        // Return a static ref — bit of a hack but works
        static INFO: std::sync::OnceLock<GpuInfo> = std::sync::OnceLock::new();
        INFO.get_or_init(|| GpuInfo {
            name: "Software Compute".into(),
            vendor: Vendor::Unknown(0),
            arch: GpuArch::Unknown,
            vram_bytes: 0,
            bus: BusType::Unknown,
        })
    }

    fn alloc(&self, size: usize) -> Result<GpuBuffer, GpuError> {
        let handle = self.next_handle.fetch_add(1, Ordering::Relaxed) as u64;
        let mut data = vec![0u8; size];
        let ptr = data.as_mut_ptr();
        let mut allocs = self.allocations.lock().unwrap();
        allocs.push(SoftAlloc { handle, data, ptr });
        Ok(GpuBuffer { handle, size, ptr })
    }

    fn write(&self, buf: &GpuBuffer, offset: usize, data: &[u8]) -> Result<(), GpuError> {
        let allocs = self.allocations.lock().unwrap();
        if let Some(alloc) = allocs.iter().find(|a| a.handle == buf.handle) {
            if offset + data.len() > alloc.data.len() {
                return Err(GpuError::AllocFailed("write oob".into()));
            }
            unsafe { ptr::copy_nonoverlapping(data.as_ptr(), alloc.ptr.add(offset), data.len()); }
            Ok(())
        } else {
            Err(GpuError::AllocFailed("buffer not found".into()))
        }
    }

    fn read(&self, buf: &GpuBuffer, offset: usize, len: usize) -> Result<Vec<u8>, GpuError> {
        let allocs = self.allocations.lock().unwrap();
        if let Some(alloc) = allocs.iter().find(|a| a.handle == buf.handle) {
            if offset + len > alloc.data.len() {
                return Err(GpuError::AllocFailed("read oob".into()));
            }
            let mut out = vec![0u8; len];
            unsafe { ptr::copy_nonoverlapping(alloc.ptr.add(offset), out.as_mut_ptr(), len); }
            Ok(out)
        } else {
            Err(GpuError::AllocFailed("buffer not found".into()))
        }
    }

    fn free(&self, buf: &GpuBuffer) -> Result<(), GpuError> {
        let mut allocs = self.allocations.lock().unwrap();
        allocs.retain(|a| a.handle != buf.handle);
        Ok(())
    }

    fn dispatch(&self, kernel: &Kernel, bufs: &[&GpuBuffer], grid: [u32; 3]) -> Result<(), GpuError> {
        if bufs.len() < 3 { return Err(GpuError::SubmitFailed("need 3 buffers".into())); }
        execute_kernel(kernel, bufs[0], bufs[1], bufs[2], grid);
        Ok(())
    }

    fn sync(&self) -> Result<(), GpuError> { Ok(()) }
}

// ── Kernel execution — parallel tile-based rendering ───────────────────────

pub fn execute_kernel(
    kernel: &Kernel,
    pixel_buf: &GpuBuffer,
    cmd_buf: &GpuBuffer,
    font_buf: &GpuBuffer,
    _grid: [u32; 3],
) {
    let width = kernel.width as usize;
    let height = kernel.height as usize;
    let cmd_count = kernel.cmd_count as usize;

    // Copy draw commands and font data into owned Vecs (Send-safe)
    let cmds: Vec<DrawCmd> = unsafe {
        let slice = std::slice::from_raw_parts(cmd_buf.ptr as *const DrawCmd, cmd_count);
        slice.to_vec()
    };

    let font_data: Vec<u8> = unsafe {
        let slice = std::slice::from_raw_parts(font_buf.ptr, font_buf.size);
        slice.to_vec()
    };

    // Tile-based parallel rendering
    let num_threads = detect_cpu_count().max(1);
    let tile_height = ((height + num_threads - 1) / num_threads).max(1);

    // Cast pointer to usize for Send safety
    let pixel_addr = pixel_buf.ptr as usize;
    let pixel_size = pixel_buf.size;

    let cmds = std::sync::Arc::new(cmds);
    let font_data = std::sync::Arc::new(font_data);

    std::thread::scope(|s| {
        for t in 0..num_threads {
            let y_start = t * tile_height;
            let y_end = ((t + 1) * tile_height).min(height);
            if y_start >= height { break; }

            let addr = pixel_addr;
            let cmds = cmds.clone();
            let font_data = font_data.clone();

            s.spawn(move || {
                let pixels: &mut [u8] = unsafe {
                    std::slice::from_raw_parts_mut(addr as *mut u8, pixel_size)
                };

                for cmd in cmds.iter() {
                    match cmd.kind {
                        0 => render_fill_rect(pixels, width, height, y_start, y_end, cmd),
                        1 => render_border_rect(pixels, width, height, y_start, y_end, cmd),
                        2 => render_glyph(pixels, width, height, y_start, y_end, cmd, &font_data),
                        _ => {}
                    }
                }
            });
        }
    });
}

// ── Per-command renderers ──────────────────────────────────────────────────

fn render_fill_rect(pixels: &mut [u8], w: usize, h: usize, y_start: usize, y_end: usize, cmd: &DrawCmd) {
    let x0 = cmd.x.max(0.0) as usize;
    let y0 = cmd.y.max(0.0) as usize;
    let x1 = ((cmd.x + cmd.w) as usize).min(w);
    let y1 = ((cmd.y + cmd.h) as usize).min(h);

    let ys = y0.max(y_start);
    let ye = y1.min(y_end);

    if cmd.a == 255 {
        for y in ys..ye {
            for x in x0..x1 {
                let i = (y * w + x) * 4;
                if i + 3 < pixels.len() {
                    pixels[i] = cmd.r;
                    pixels[i + 1] = cmd.g;
                    pixels[i + 2] = cmd.b;
                    pixels[i + 3] = 255;
                }
            }
        }
    } else if cmd.a > 0 {
        let alpha = cmd.a as f32 / 255.0;
        let inv = 1.0 - alpha;
        for y in ys..ye {
            for x in x0..x1 {
                let i = (y * w + x) * 4;
                if i + 3 < pixels.len() {
                    pixels[i] = (cmd.r as f32 * alpha + pixels[i] as f32 * inv) as u8;
                    pixels[i + 1] = (cmd.g as f32 * alpha + pixels[i + 1] as f32 * inv) as u8;
                    pixels[i + 2] = (cmd.b as f32 * alpha + pixels[i + 2] as f32 * inv) as u8;
                    pixels[i + 3] = 255;
                }
            }
        }
    }
}

fn render_border_rect(pixels: &mut [u8], w: usize, h: usize, y_start: usize, y_end: usize, cmd: &DrawCmd) {
    let thickness = (cmd.extra as f32 / 100.0).max(1.0);
    let t = thickness as usize;

    // Top edge
    let mut top = *cmd;
    top.kind = 0;
    top.h = thickness;
    render_fill_rect(pixels, w, h, y_start, y_end, &top);

    // Bottom edge
    let mut bottom = *cmd;
    bottom.kind = 0;
    bottom.y = cmd.y + cmd.h - thickness;
    bottom.h = thickness;
    render_fill_rect(pixels, w, h, y_start, y_end, &bottom);

    // Left edge
    let mut left = *cmd;
    left.kind = 0;
    left.w = thickness;
    render_fill_rect(pixels, w, h, y_start, y_end, &left);

    // Right edge
    let mut right = *cmd;
    right.kind = 0;
    right.x = cmd.x + cmd.w - thickness;
    right.w = thickness;
    render_fill_rect(pixels, w, h, y_start, y_end, &right);
}

fn render_glyph(pixels: &mut [u8], w: usize, h: usize, y_start: usize, y_end: usize, cmd: &DrawCmd, font_data: &[u8]) {
    let ch = cmd.extra;
    let font_size = cmd.font_size;
    let scale = font_size / 7.0;

    // Look up glyph in font atlas
    // Font atlas format: 7 bytes per character, indexed by char code
    // Characters 32-126 (printable ASCII) = indices 0-94
    let glyph_idx = if ch >= 32 && ch <= 126 {
        (ch - 32) as usize
    } else {
        94 // unknown = last entry (box glyph)
    };

    let glyph_offset = glyph_idx * 7;
    if glyph_offset + 7 > font_data.len() { return; }

    let bitmap = &font_data[glyph_offset..glyph_offset + 7];

    for row in 0..7u32 {
        for col in 0..5u32 {
            if bitmap[row as usize] & (1 << (4 - col)) != 0 {
                let px = cmd.x + col as f32 * scale;
                let py = cmd.y + row as f32 * scale;
                let s = scale.ceil() as usize;

                for dy in 0..s {
                    let y = py as usize + dy;
                    if y < y_start || y >= y_end { continue; }
                    for dx in 0..s {
                        let x = px as usize + dx;
                        if x >= w || y >= h { continue; }
                        let i = (y * w + x) * 4;
                        if i + 3 < pixels.len() {
                            if cmd.a == 255 {
                                pixels[i] = cmd.r;
                                pixels[i + 1] = cmd.g;
                                pixels[i + 2] = cmd.b;
                                pixels[i + 3] = 255;
                            } else {
                                let alpha = cmd.a as f32 / 255.0;
                                let inv = 1.0 - alpha;
                                pixels[i] = (cmd.r as f32 * alpha + pixels[i] as f32 * inv) as u8;
                                pixels[i + 1] = (cmd.g as f32 * alpha + pixels[i + 1] as f32 * inv) as u8;
                                pixels[i + 2] = (cmd.b as f32 * alpha + pixels[i + 2] as f32 * inv) as u8;
                                pixels[i + 3] = 255;
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── CPU core detection (no libraries) ──────────────────────────────────────

fn detect_cpu_count() -> usize {
    #[cfg(target_os = "linux")]
    {
        // Read /sys/devices/system/cpu/online or /proc/cpuinfo
        if let Ok(s) = std::fs::read_to_string("/sys/devices/system/cpu/online") {
            // Format: "0-7" or "0-3,5-7"
            if let Some(max) = s.trim().split('-').last().and_then(|n| n.parse::<usize>().ok()) {
                return max + 1;
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn sysctlbyname(name: *const u8, oldp: *mut std::ffi::c_void, oldlenp: *mut usize, newp: *const std::ffi::c_void, newlen: usize) -> i32;
        }
        let name = b"hw.ncpu\0";
        let mut ncpu: i32 = 0;
        let mut size = std::mem::size_of::<i32>();
        unsafe {
            if sysctlbyname(name.as_ptr(), &mut ncpu as *mut i32 as *mut _, &mut size, std::ptr::null(), 0) == 0 {
                return ncpu as usize;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // GetSystemInfo — or read from PEB
        // For simplicity, use std::thread::available_parallelism
    }

    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}
