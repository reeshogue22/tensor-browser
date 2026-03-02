/// Linux GPU backend — raw DRM ioctls.
/// No libdrm. No Mesa. No EGL. Just open() + ioctl() on /dev/dri/renderD*.
/// Struct layouts and ioctl numbers defined by hand from kernel headers.

use super::*;
use super::compute;
use std::ptr;

// ── Raw syscall wrappers ───────────────────────────────────────────────────
// We use libc for open/close/ioctl/mmap since these ARE the syscalls
// (thin wrappers around SYS_open, SYS_ioctl, SYS_mmap)

// DRM ioctl command encoding
// _IO(type, nr) = type << 8 | nr
// _IOR(type, nr, size) = 0x80000000 | size << 16 | type << 8 | nr
// _IOW(type, nr, size) = 0x40000000 | size << 16 | type << 8 | nr
// _IOWR(type, nr, size) = 0xC0000000 | size << 16 | type << 8 | nr
const DRM_IOCTL_BASE: u32 = b'd' as u32;

const fn drm_io(nr: u32) -> u64 {
    ((DRM_IOCTL_BASE << 8) | nr) as u64
}

const fn drm_iowr(nr: u32, size: u32) -> u64 {
    (0xC0000000 | (size << 16) | (DRM_IOCTL_BASE << 8) | nr) as u64
}

const fn drm_iow(nr: u32, size: u32) -> u64 {
    (0x40000000 | (size << 16) | (DRM_IOCTL_BASE << 8) | nr) as u64
}

const fn drm_ior(nr: u32, size: u32) -> u64 {
    (0x80000000u32 | (size << 16) | (DRM_IOCTL_BASE << 8) | nr) as u64
}

// DRM ioctl numbers
const DRM_IOCTL_VERSION: u64 = drm_iowr(0x00, 4 * 4 + 3 * 8 + 3 * 8); // struct drm_version
const DRM_IOCTL_GET_CAP: u64 = drm_iowr(0x0C, 16); // struct drm_get_cap

// DRM dumb buffer ioctls
const DRM_IOCTL_MODE_CREATE_DUMB: u64 = drm_iowr(0xB2, 32); // struct drm_mode_create_dumb
const DRM_IOCTL_MODE_MAP_DUMB: u64 = drm_iowr(0xB3, 16);    // struct drm_mode_map_dumb
const DRM_IOCTL_MODE_DESTROY_DUMB: u64 = drm_iowr(0xB4, 4);  // struct drm_mode_destroy_dumb

// DRM PRIME (dma-buf) ioctls for GPU buffer sharing
const DRM_IOCTL_PRIME_HANDLE_TO_FD: u64 = drm_iowr(0x2D, 12);
const DRM_IOCTL_PRIME_FD_TO_HANDLE: u64 = drm_iowr(0x2E, 12);

const DRM_CAP_DUMB_BUFFER: u64 = 0x01;
const DRM_CAP_PRIME: u64 = 0x05;

// mmap flags
const PROT_READ: i32 = 0x01;
const PROT_WRITE: i32 = 0x02;
const MAP_SHARED: i32 = 0x01;

// ── DRM structs (from kernel UAPI, hand-defined) ───────────────────────────

#[repr(C)]
struct DrmGetCap {
    capability: u64,
    value: u64,
}

#[repr(C)]
struct DrmModeCreateDumb {
    height: u32,
    width: u32,
    bpp: u32,
    flags: u32,
    // Output
    handle: u32,
    pitch: u32,
    size: u64,
}

#[repr(C)]
struct DrmModeMapDumb {
    handle: u32,
    pad: u32,
    offset: u64,
}

#[repr(C)]
struct DrmModeDestroyDumb {
    handle: u32,
}

#[repr(C)]
#[allow(dead_code)]
struct DrmVersion {
    version_major: i32,
    version_minor: i32,
    version_patchlevel: i32,
    name_len: u64,
    name: *mut u8,
    date_len: u64,
    date: *mut u8,
    desc_len: u64,
    desc: *mut u8,
}

// ── DRM Device ─────────────────────────────────────────────────────────────

pub struct DrmDevice {
    info: GpuInfo,
    fd: i32,
    driver_name: String,
    allocations: std::sync::Mutex<Vec<DrmAlloc>>,
}

struct DrmAlloc {
    handle: u32,
    ptr: *mut u8,
    size: usize,
    mmap_offset: u64,
}

unsafe impl Send for DrmAlloc {}
unsafe impl Sync for DrmAlloc {}

impl DrmDevice {
    pub fn open(gpu: &GpuInfo) -> Result<Self, GpuError> {
        // Try render nodes /dev/dri/renderD128..renderD135
        for i in 128..136 {
            let path = format!("/dev/dri/renderD{}\0", i);
            let fd = unsafe {
                libc::open(path.as_ptr() as *const i8, libc::O_RDWR)
            };
            if fd < 0 { continue; }

            // Get driver version
            let mut name_buf = [0u8; 128];
            let mut date_buf = [0u8; 64];
            let mut desc_buf = [0u8; 256];
            let mut version = DrmVersion {
                version_major: 0,
                version_minor: 0,
                version_patchlevel: 0,
                name_len: name_buf.len() as u64,
                name: name_buf.as_mut_ptr(),
                date_len: date_buf.len() as u64,
                date: date_buf.as_mut_ptr(),
                desc_len: desc_buf.len() as u64,
                desc: desc_buf.as_mut_ptr(),
            };

            let ret = unsafe {
                libc::ioctl(fd, DRM_IOCTL_VERSION, &mut version as *mut _ as *mut libc::c_void)
            };

            let driver_name = if ret == 0 {
                let len = version.name_len as usize;
                String::from_utf8_lossy(&name_buf[..len]).to_string()
            } else {
                "unknown".to_string()
            };

            // Check if dumb buffers are supported
            let mut cap = DrmGetCap { capability: DRM_CAP_DUMB_BUFFER, value: 0 };
            unsafe {
                libc::ioctl(fd, DRM_IOCTL_GET_CAP, &mut cap as *mut _ as *mut libc::c_void);
            }
            let has_dumb = cap.value != 0;

            if !has_dumb {
                unsafe { libc::close(fd); }
                continue;
            }

            return Ok(DrmDevice {
                info: gpu.clone(),
                fd,
                driver_name,
                allocations: std::sync::Mutex::new(Vec::new()),
            });
        }

        Err(GpuError::NoDevice)
    }
}

impl GpuDevice for DrmDevice {
    fn info(&self) -> &GpuInfo { &self.info }

    fn alloc(&self, size: usize) -> Result<GpuBuffer, GpuError> {
        // Create a dumb buffer — this allocates GPU-visible memory via kernel driver
        // We use width=size/4, height=1, bpp=32 to get the right byte count
        let width = ((size + 3) / 4).max(1) as u32;
        let mut create = DrmModeCreateDumb {
            height: 1,
            width,
            bpp: 32,
            flags: 0,
            handle: 0,
            pitch: 0,
            size: 0,
        };

        let ret = unsafe {
            libc::ioctl(self.fd, DRM_IOCTL_MODE_CREATE_DUMB, &mut create as *mut _ as *mut libc::c_void)
        };
        if ret != 0 {
            return Err(GpuError::SyscallFailed("DRM_IOCTL_MODE_CREATE_DUMB".into(),
                unsafe { *libc::__errno_location() } as i64));
        }

        // Map the dumb buffer into our address space
        let mut map = DrmModeMapDumb {
            handle: create.handle,
            pad: 0,
            offset: 0,
        };

        let ret = unsafe {
            libc::ioctl(self.fd, DRM_IOCTL_MODE_MAP_DUMB, &mut map as *mut _ as *mut libc::c_void)
        };
        if ret != 0 {
            return Err(GpuError::SyscallFailed("DRM_IOCTL_MODE_MAP_DUMB".into(),
                unsafe { *libc::__errno_location() } as i64));
        }

        // mmap the buffer
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                create.size as usize,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                self.fd,
                map.offset as i64,
            ) as *mut u8
        };

        if ptr == libc::MAP_FAILED as *mut u8 {
            return Err(GpuError::MapFailed);
        }

        let handle = create.handle as u64;
        let alloc_size = create.size as usize;

        let mut allocs = self.allocations.lock().unwrap();
        allocs.push(DrmAlloc {
            handle: create.handle,
            ptr,
            size: alloc_size,
            mmap_offset: map.offset,
        });

        Ok(GpuBuffer {
            handle,
            size: alloc_size,
            ptr,
        })
    }

    fn write(&self, buf: &GpuBuffer, offset: usize, data: &[u8]) -> Result<(), GpuError> {
        if buf.ptr.is_null() { return Err(GpuError::MapFailed); }
        if offset + data.len() > buf.size { return Err(GpuError::AllocFailed("write oob".into())); }
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), buf.ptr.add(offset), data.len()); }
        Ok(())
    }

    fn read(&self, buf: &GpuBuffer, offset: usize, len: usize) -> Result<Vec<u8>, GpuError> {
        if buf.ptr.is_null() { return Err(GpuError::MapFailed); }
        if offset + len > buf.size { return Err(GpuError::AllocFailed("read oob".into())); }
        let mut data = vec![0u8; len];
        unsafe { ptr::copy_nonoverlapping(buf.ptr.add(offset), data.as_mut_ptr(), len); }
        Ok(data)
    }

    fn free(&self, buf: &GpuBuffer) -> Result<(), GpuError> {
        let mut allocs = self.allocations.lock().unwrap();
        if let Some(idx) = allocs.iter().position(|a| a.handle as u64 == buf.handle) {
            let alloc = allocs.remove(idx);
            unsafe {
                libc::munmap(alloc.ptr as *mut libc::c_void, alloc.size);
                let mut destroy = DrmModeDestroyDumb { handle: alloc.handle };
                libc::ioctl(self.fd, DRM_IOCTL_MODE_DESTROY_DUMB, &mut destroy as *mut _ as *mut libc::c_void);
            }
        }
        Ok(())
    }

    fn dispatch(&self, kernel: &compute::Kernel, bufs: &[&GpuBuffer], grid: [u32; 3]) -> Result<(), GpuError> {
        if bufs.len() < 3 {
            return Err(GpuError::SubmitFailed("need pixel, cmd, font buffers".into()));
        }
        // Execute on CPU using the DRM-allocated (GPU-visible) memory
        // For actual GPU dispatch, we'd build a GEM command buffer
        // with the appropriate driver-specific format (amdgpu, i915, etc.)
        compute::execute_kernel(kernel, bufs[0], bufs[1], bufs[2], grid);
        Ok(())
    }

    fn sync(&self) -> Result<(), GpuError> {
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

impl Drop for DrmDevice {
    fn drop(&mut self) {
        let allocs = self.allocations.lock().unwrap();
        for alloc in allocs.iter() {
            unsafe {
                libc::munmap(alloc.ptr as *mut libc::c_void, alloc.size);
                let mut destroy = DrmModeDestroyDumb { handle: alloc.handle };
                libc::ioctl(self.fd, DRM_IOCTL_MODE_DESTROY_DUMB, &mut destroy as *mut _ as *mut libc::c_void);
            }
        }
        unsafe { libc::close(self.fd); }
    }
}
