use iced_x86::{
    BlockEncoder, BlockEncoderOptions, Decoder, DecoderOptions, FlowControl, InstructionBlock,
};
use std::ffi::c_void;
use std::sync::Mutex;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

const JUMP_LEN: usize = 14;
const POOL_SIZE: usize = 0x10000;

struct Pool {
    start: usize,
    used: usize,
}

static POOLS: Mutex<Vec<Pool>> = Mutex::new(Vec::new());

fn alloc_near(target: usize, size: usize) -> Option<usize> {
    let mut pools = POOLS.lock().ok()?;
    for p in pools.iter_mut() {
        if p.used + size <= POOL_SIZE && p.start.abs_diff(target) < 0x7000_0000 {
            let at = p.start + p.used;
            p.used += (size + 15) & !15;
            return Some(at);
        }
    }
    let step = 0x10000usize;
    let lo = target.saturating_sub(0x7000_0000) & !(step - 1);
    let hi = target.saturating_add(0x7000_0000);
    let mut addr = target & !(step - 1);
    let mut down = true;
    let mut up_addr = addr;
    loop {
        let try_at = if down {
            addr = addr.checked_sub(step)?;
            if addr < lo {
                down = false;
                continue;
            }
            addr
        } else {
            up_addr += step;
            if up_addr > hi {
                return None;
            }
            up_addr
        };
        let p = unsafe {
            VirtualAlloc(
                try_at as *const c_void,
                POOL_SIZE,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            )
        } as usize;
        if p != 0 {
            pools.push(Pool {
                start: p,
                used: (size + 15) & !15,
            });
            return Some(p);
        }
    }
}

pub unsafe fn write_code(at: usize, bytes: &[u8]) -> bool {
    let mut old = 0u32;
    unsafe {
        if VirtualProtect(
            at as *const c_void,
            bytes.len(),
            PAGE_EXECUTE_READWRITE,
            &mut old,
        ) == 0
        {
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), at as *mut u8, bytes.len());
        VirtualProtect(at as *const c_void, bytes.len(), old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), at as *const c_void, bytes.len());
    }
    true
}

pub fn abs_jump(target: usize) -> [u8; JUMP_LEN] {
    let mut j = [0xFF, 0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    j[6..].copy_from_slice(&(target as u64).to_le_bytes());
    j
}

/// Redirects `target` to `hook`. Returns the address of a trampoline that runs the original code.
pub unsafe fn install(target: usize, hook: usize) -> Result<usize, String> {
    let code = unsafe { std::slice::from_raw_parts(target as *const u8, 64) };
    let mut decoder = Decoder::with_ip(64, code, target as u64, DecoderOptions::NONE);
    let mut stolen = Vec::new();
    let mut len = 0usize;
    while len < JUMP_LEN {
        let ins = decoder.decode();
        if ins.is_invalid() {
            return Err(format!("cannot decode instruction at {:#x}", target + len));
        }
        if len + ins.len() < JUMP_LEN
            && matches!(
                ins.flow_control(),
                FlowControl::Return | FlowControl::UnconditionalBranch | FlowControl::Interrupt
            )
        {
            return Err(format!("function at {target:#x} is too short to hook"));
        }
        len += ins.len();
        stolen.push(ins);
    }
    let tramp = alloc_near(target, 128).ok_or("no memory near the target")?;
    let block = InstructionBlock::new(&stolen, tramp as u64);
    let encoded = BlockEncoder::encode(64, block, BlockEncoderOptions::NONE)
        .map_err(|e| format!("cannot relocate {target:#x}: {e}"))?;
    let mut bytes = encoded.code_buffer;
    if bytes.len() + JUMP_LEN > 128 {
        return Err("relocated prologue too long".into());
    }
    bytes.extend_from_slice(&abs_jump(target + len));
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), tramp as *mut u8, bytes.len()) };
    let mut patch = vec![0x90u8; len];
    patch[..JUMP_LEN].copy_from_slice(&abs_jump(hook));
    if !unsafe { write_code(target, &patch) } {
        return Err(format!("cannot write to {target:#x}"));
    }
    Ok(tramp)
}
