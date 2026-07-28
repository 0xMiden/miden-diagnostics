#![no_main]
#![no_std]

use core::{
    alloc::{GlobalAlloc, Layout},
    panic::PanicInfo,
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

struct BumpAllocator;

const HEAP_SIZE: usize = 64 * 1024;
static NEXT: AtomicUsize = AtomicUsize::new(0);
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let base = ptr::addr_of_mut!(HEAP).cast::<u8>();
        let base_address = base.addr();
        let reserved = NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            let address = base_address.checked_add(next)?;
            let aligned_address = address.checked_add(layout.align() - 1)? & !(layout.align() - 1);
            let aligned = aligned_address.checked_sub(base_address)?;
            let end = aligned.checked_add(layout.size())?;
            (end <= HEAP_SIZE).then_some(end)
        });

        match reserved {
            Ok(previous) => {
                let address = base_address + previous;
                let aligned_address = (address + layout.align() - 1) & !(layout.align() - 1);
                let offset = aligned_address - base_address;
                // SAFETY: the checked monotonic reservation above gives this
                // allocation a unique in-bounds region of the static heap.
                unsafe { base.add(offset) }
            }
            Err(_) => ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, _pointer: *mut u8, _layout: Layout) {}
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> usize {
    no_std_consumer::exercise()
}

#[unsafe(no_mangle)]
pub extern "C" fn registry_probe() -> i32 {
    no_std_consumer::registry_probe()
}
