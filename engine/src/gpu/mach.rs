/// macOS GPU backend — raw Mach syscalls.
/// No Metal. No IOKit framework. No CoreFoundation.
/// Just mach_vm_allocate/deallocate (Mach traps) for unified memory.

use super::*;
use super::compute;
use std::ptr;

// ── Raw Mach types ─────────────────────────────────────────────────────────

type MachPort = u32;
type KernReturn = i32;

const KERN_SUCCESS: KernReturn = 0;
const MACH_PORT_NULL: MachPort = 0;

// ── Syscall declarations (libSystem — these ARE the raw kernel traps) ──────

unsafe extern "C" {
    fn mach_task_self() -> MachPort;

    fn mach_vm_allocate(
        target: MachPort,
        address: *mut u64,
        size: u64,
        flags: i32,
    ) -> KernReturn;

    fn mach_vm_deallocate(
        target: MachPort,
        address: u64,
        size: u64,
    ) -> KernReturn;

    fn sysctlbyname(
        name: *const u8,
        oldp: *mut std::ffi::c_void,
        oldlenp: *mut usize,
        newp: *const std::ffi::c_void,
        newlen: usize,
    ) -> i32;
}

// ── Mach GPU Device ────────────────────────────────────────────────────────

pub struct MachDevice {
    info: GpuInfo,
    task: MachPort,
    allocations: std::sync::Mutex<Vec<MachAlloc>>,
}

struct MachAlloc {
    handle: u64,
    size: usize,
    ptr: *mut u8,
}

unsafe impl Send for MachAlloc {}
unsafe impl Sync for MachAlloc {}

impl MachDevice {
    pub fn open(gpu: &GpuInfo) -> Result<Self, GpuError> {
        let task = unsafe { mach_task_self() };
        if task == MACH_PORT_NULL {
            return Err(GpuError::SyscallFailed("mach_task_self".into(), 0));
        }

        Ok(MachDevice {
            info: gpu.clone(),
            task,
            allocations: std::sync::Mutex::new(Vec::new()),
        })
    }
}

impl GpuDevice for MachDevice {
    fn info(&self) -> &GpuInfo { &self.info }

    fn alloc(&self, size: usize) -> Result<GpuBuffer, GpuError> {
        unsafe {
            let mut address: u64 = 0;
            let page_size = 16384u64; // Apple Silicon = 16KB pages
            let aligned = ((size as u64 + page_size - 1) / page_size) * page_size;

            let kr = mach_vm_allocate(self.task, &mut address, aligned, 1 /* VM_FLAGS_ANYWHERE */);
            if kr != KERN_SUCCESS {
                return Err(GpuError::SyscallFailed("mach_vm_allocate".into(), kr as i64));
            }

            let ptr = address as *mut u8;
            let mut allocs = self.allocations.lock().unwrap();
            allocs.push(MachAlloc { handle: address, size: aligned as usize, ptr });

            Ok(GpuBuffer { handle: address, size: aligned as usize, ptr })
        }
    }

    fn write(&self, buf: &GpuBuffer, offset: usize, data: &[u8]) -> Result<(), GpuError> {
        if buf.ptr.is_null() { return Err(GpuError::MapFailed); }
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), buf.ptr.add(offset), data.len()); }
        Ok(())
    }

    fn read(&self, buf: &GpuBuffer, offset: usize, len: usize) -> Result<Vec<u8>, GpuError> {
        if buf.ptr.is_null() { return Err(GpuError::MapFailed); }
        let mut out = vec![0u8; len];
        unsafe { ptr::copy_nonoverlapping(buf.ptr.add(offset), out.as_mut_ptr(), len); }
        Ok(out)
    }

    fn free(&self, buf: &GpuBuffer) -> Result<(), GpuError> {
        let mut allocs = self.allocations.lock().unwrap();
        if let Some(idx) = allocs.iter().position(|a| a.handle == buf.handle) {
            let alloc = allocs.remove(idx);
            unsafe { mach_vm_deallocate(self.task, alloc.handle, alloc.size as u64); }
        }
        Ok(())
    }

    fn dispatch(&self, kernel: &compute::Kernel, bufs: &[&GpuBuffer], grid: [u32; 3]) -> Result<(), GpuError> {
        if bufs.len() < 3 { return Err(GpuError::SubmitFailed("need 3 buffers".into())); }
        compute::execute_kernel(kernel, bufs[0], bufs[1], bufs[2], grid);
        Ok(())
    }

    fn sync(&self) -> Result<(), GpuError> {
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

impl Drop for MachDevice {
    fn drop(&mut self) {
        let allocs = self.allocations.lock().unwrap();
        for alloc in allocs.iter() {
            unsafe { mach_vm_deallocate(self.task, alloc.handle, alloc.size as u64); }
        }
    }
}
