/// GPU — universal hardware access layer.
/// Detects GPU hardware directly:
///   x86 Linux:        /sys/bus/pci scan
///   x86 Windows:      PCI config space (port 0xCF8/0xCFC)
///   Apple Silicon:    MMIO probing (Asahi-style, known SoC addresses)
///   ARM Linux:        device tree (/proc/device-tree/)
/// Then talks to it via raw syscalls:
///   Linux:            ioctl() on /dev/dri/renderD*
///   macOS:            mach_msg() to IOKit (raw Mach IPC, no framework)
///   Windows:          NtDeviceIoControlFile() on GPU device
///
/// No Metal. No Vulkan. No OpenGL. No DirectX.
/// No frameworks. No libraries. Just syscalls and hardware.

pub mod probe;
pub mod compute;

#[cfg(target_os = "linux")]
pub mod drm;
#[cfg(target_os = "macos")]
pub mod mach;
#[cfg(target_os = "windows")]
pub mod nt;

use std::fmt;

// ── GPU Hardware Info ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: Vendor,
    pub arch: GpuArch,
    pub vram_bytes: u64,
    pub bus: BusType,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Vendor {
    Apple,
    Amd,
    Nvidia,
    Intel,
    Unknown(u16),
}

impl Vendor {
    pub fn from_pci_id(id: u16) -> Self {
        match id {
            0x106B => Vendor::Apple,
            0x1002 => Vendor::Amd,
            0x10DE => Vendor::Nvidia,
            0x8086 => Vendor::Intel,
            other => Vendor::Unknown(other),
        }
    }
}

impl fmt::Display for Vendor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Vendor::Apple => write!(f, "Apple"),
            Vendor::Amd => write!(f, "AMD"),
            Vendor::Nvidia => write!(f, "NVIDIA"),
            Vendor::Intel => write!(f, "Intel"),
            Vendor::Unknown(id) => write!(f, "Unknown(0x{:04x})", id),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum GpuArch {
    AppleAgx,       // Apple GPU (M1/M2/M3/M4)
    AmdRdna,        // AMD RDNA 1/2/3
    AmdGcn,         // AMD GCN
    NvidiaAmpere,   // NVIDIA Ampere
    NvidiaTuring,   // NVIDIA Turing
    IntelXe,        // Intel Xe
    IntelGen,       // Intel Gen9/Gen11
    Unknown,
}

#[derive(Clone, Debug)]
pub enum BusType {
    Pci { bus: u8, device: u8, function: u8, bars: [u64; 6] },
    SoC { mmio_base: u64, mmio_size: u64 },
    Unknown,
}

// ── Device Trait ────────────────────────────────────────────────────────────

pub trait GpuDevice: Send + Sync {
    fn info(&self) -> &GpuInfo;
    fn alloc(&self, size: usize) -> Result<GpuBuffer, GpuError>;
    fn write(&self, buf: &GpuBuffer, offset: usize, data: &[u8]) -> Result<(), GpuError>;
    fn read(&self, buf: &GpuBuffer, offset: usize, len: usize) -> Result<Vec<u8>, GpuError>;
    fn free(&self, buf: &GpuBuffer) -> Result<(), GpuError>;
    fn dispatch(&self, kernel: &compute::Kernel, bufs: &[&GpuBuffer], grid: [u32; 3]) -> Result<(), GpuError>;
    fn sync(&self) -> Result<(), GpuError>;
}

#[derive(Debug)]
pub struct GpuBuffer {
    pub handle: u64,
    pub size: usize,
    pub ptr: *mut u8,
}

unsafe impl Send for GpuBuffer {}
unsafe impl Sync for GpuBuffer {}

#[derive(Debug)]
pub enum GpuError {
    NoDevice,
    ProbeError(String),
    AllocFailed(String),
    MapFailed,
    SubmitFailed(String),
    SyscallFailed(String, i64),
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuError::NoDevice => write!(f, "no GPU found"),
            GpuError::ProbeError(s) => write!(f, "probe: {}", s),
            GpuError::AllocFailed(s) => write!(f, "alloc: {}", s),
            GpuError::MapFailed => write!(f, "mmap failed"),
            GpuError::SubmitFailed(s) => write!(f, "submit: {}", s),
            GpuError::SyscallFailed(name, errno) => write!(f, "syscall {} failed: errno {}", name, errno),
        }
    }
}

// ── Draw Commands ───────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DrawCmd {
    pub kind: u32,       // 0=fill_rect, 1=border_rect, 2=glyph
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
    pub extra: u32,      // glyph: char_code | border: thickness*100
    pub font_size: f32,
    pub _pad: u32,
}

impl DrawCmd {
    pub fn fill_rect(x: f32, y: f32, w: f32, h: f32, r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { kind: 0, x, y, w, h, r, g, b, a, extra: 0, font_size: 0.0, _pad: 0 }
    }
    pub fn border_rect(x: f32, y: f32, w: f32, h: f32, r: u8, g: u8, b: u8, a: u8, thickness: f32) -> Self {
        Self { kind: 1, x, y, w, h, r, g, b, a, extra: (thickness * 100.0) as u32, font_size: 0.0, _pad: 0 }
    }
    pub fn glyph(x: f32, y: f32, font_size: f32, ch: char, r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { kind: 2, x, y, w: 0.0, h: 0.0, r, g, b, a, extra: ch as u32, font_size, _pad: 0 }
    }
}

// ── Collect draw commands from layout tree ──────────────────────────────────

use crate::css::Color;
use crate::layout::{LayoutBox, BoxKind};

pub fn collect_draw_commands(layout_box: &LayoutBox) -> Vec<DrawCmd> {
    let mut cmds = Vec::new();
    collect_box(&mut cmds, layout_box);
    cmds
}

fn collect_box(cmds: &mut Vec<DrawCmd>, lb: &LayoutBox) {
    let bg = lb.style.background_color();
    if bg.a > 0 {
        cmds.push(DrawCmd::fill_rect(
            lb.rect.x - lb.padding.left, lb.rect.y - lb.padding.top,
            lb.rect.width + lb.padding.left + lb.padding.right,
            lb.rect.height + lb.padding.top + lb.padding.bottom,
            bg.r, bg.g, bg.b, bg.a,
        ));
    }
    let bw = lb.border.top;
    if bw > 0.0 {
        let bc = match lb.style.get("border-top-color") {
            Some(crate::css::CssValue::Color(c)) => *c,
            _ => Color::BLACK,
        };
        cmds.push(DrawCmd::border_rect(
            lb.rect.x - lb.padding.left - lb.border.left,
            lb.rect.y - lb.padding.top - lb.border.top,
            lb.rect.width + lb.padding.left + lb.padding.right + lb.border.left + lb.border.right,
            lb.rect.height + lb.padding.top + lb.padding.bottom + lb.border.top + lb.border.bottom,
            bc.r, bc.g, bc.b, bc.a, bw,
        ));
    }
    // Flex and InlineBlock paint same as Block (bg + border already handled above)
    if let BoxKind::Text(text) = &lb.kind {
        let color = lb.style.color();
        let fs = lb.style.font_size();
        let cw = fs * 0.6;
        let mut cx = lb.rect.x;
        for ch in text.chars() {
            cmds.push(DrawCmd::glyph(cx, lb.rect.y, fs, ch, color.r, color.g, color.b, color.a));
            cx += cw;
        }
        if lb.style.text_decoration() == "underline" {
            cmds.push(DrawCmd::fill_rect(lb.rect.x, lb.rect.y + fs + 1.0, lb.rect.width, 1.0, color.r, color.g, color.b, color.a));
        }
    }
    for child in &lb.children {
        collect_box(cmds, child);
    }
}

// ── Open best available device ──────────────────────────────────────────────

pub fn open_device() -> Result<Box<dyn GpuDevice>, GpuError> {
    // 1. Probe hardware
    let gpus = probe::scan();
    if gpus.is_empty() {
        println!("  gpu: no hardware found, using software compute");
        return Ok(Box::new(compute::SoftDevice::new()));
    }

    let gpu = &gpus[0];
    println!("  gpu: found {} ({}) via {:?}", gpu.name, gpu.vendor, gpu.bus);

    // 2. Open platform-specific device
    #[cfg(target_os = "linux")]
    {
        match drm::DrmDevice::open(gpu) {
            Ok(dev) => return Ok(Box::new(dev)),
            Err(e) => println!("  gpu: DRM open failed: {}", e),
        }
    }

    #[cfg(target_os = "macos")]
    {
        match mach::MachDevice::open(gpu) {
            Ok(dev) => return Ok(Box::new(dev)),
            Err(e) => println!("  gpu: Mach open failed: {}", e),
        }
    }

    #[cfg(target_os = "windows")]
    {
        match nt::NtDevice::open(gpu) {
            Ok(dev) => return Ok(Box::new(dev)),
            Err(e) => println!("  gpu: NT open failed: {}", e),
        }
    }

    // 3. Fallback to software
    println!("  gpu: falling back to software compute");
    Ok(Box::new(compute::SoftDevice::new()))
}

/// GPU-accelerated paint
pub fn gpu_paint(layout_root: &LayoutBox, width: u32, height: u32) -> crate::render::Canvas {
    let device = match open_device() {
        Ok(d) => d,
        Err(_) => Box::new(compute::SoftDevice::new()),
    };

    let cmds = collect_draw_commands(layout_root);
    println!("  gpu: {} draw commands", cmds.len());

    let font_atlas = crate::render::build_font_atlas();
    let pixel_size = (width * height * 4) as usize;
    let cmd_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(cmds.as_ptr() as *const u8, cmds.len() * std::mem::size_of::<DrawCmd>())
    };

    let pixel_buf = device.alloc(pixel_size).unwrap();
    let cmd_buf = device.alloc(cmd_bytes.len()).unwrap();
    let font_buf = device.alloc(font_atlas.len()).unwrap();

    // White background
    device.write(&pixel_buf, 0, &vec![255u8; pixel_size]).unwrap();
    device.write(&cmd_buf, 0, cmd_bytes).unwrap();
    device.write(&font_buf, 0, &font_atlas).unwrap();

    let kernel = compute::Kernel { width, height, cmd_count: cmds.len() as u32 };
    device.dispatch(&kernel, &[&pixel_buf, &cmd_buf, &font_buf], [width, height, 1]).unwrap();
    device.sync().unwrap();

    let pixels = device.read(&pixel_buf, 0, pixel_size).unwrap();
    device.free(&pixel_buf).ok();
    device.free(&cmd_buf).ok();
    device.free(&font_buf).ok();

    crate::render::Canvas { pixels, width, height }
}
