use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

extern "system" fn not_implemented() -> i32 {
    0x8000_4001u32 as i32
}

macro_rules! forward {
    ($($name:ident => $slot:ident),* $(,)?) => {
        $(
            static mut $slot: usize = 0;

            #[unsafe(naked)]
            #[unsafe(no_mangle)]
            pub extern "system" fn $name() {
                core::arch::naked_asm!("jmp qword ptr [rip + {0}]", sym $slot);
            }
        )*

        pub fn load_real_dxgi() {
            let mut buf = [0u16; 260];
            let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
            let mut path: Vec<u16> = buf[..n.min(buf.len())].to_vec();
            path.extend("\\dxgi.dll".encode_utf16());
            path.push(0);
            let real = unsafe { LoadLibraryW(path.as_ptr()) };
            if real.is_null() {
                crate::log::write("real dxgi.dll not found");
            }
            $(
                let addr = if real.is_null() {
                    None
                } else {
                    unsafe { GetProcAddress(real, concat!(stringify!($name), "\0").as_ptr()) }
                };
                unsafe { $slot = addr.map_or(not_implemented as *const () as usize, |f| f as usize) };
            )*
        }
    };
}

forward! {
    CreateDXGIFactory => REAL_CREATE_DXGI_FACTORY,
    CreateDXGIFactory1 => REAL_CREATE_DXGI_FACTORY1,
    CreateDXGIFactory2 => REAL_CREATE_DXGI_FACTORY2,
    DXGID3D10CreateDevice => REAL_D3D10_CREATE_DEVICE,
    DXGID3D10CreateLayeredDevice => REAL_D3D10_CREATE_LAYERED_DEVICE,
    DXGID3D10GetLayeredDeviceSize => REAL_D3D10_GET_LAYERED_DEVICE_SIZE,
    DXGID3D10RegisterLayers => REAL_D3D10_REGISTER_LAYERS,
    DXGIDeclareAdapterRemovalSupport => REAL_DECLARE_ADAPTER_REMOVAL_SUPPORT,
    DXGIDisableVBlankVirtualization => REAL_DISABLE_VBLANK_VIRTUALIZATION,
    DXGIDumpJournal => REAL_DUMP_JOURNAL,
    DXGIGetDebugInterface1 => REAL_GET_DEBUG_INTERFACE1,
    DXGIReportAdapterConfiguration => REAL_REPORT_ADAPTER_CONFIGURATION,
}
