use super::process::abort_with_message;
use libc::{c_int, c_void, memcpy, size_t};
use wasmer_runtime_core::{
    units::{Pages, WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE},
    vm::Ctx,
};

/// emscripten: _emscripten_memcpy_big
pub fn _emscripten_memcpy_big(ctx: &mut Ctx, dest: u32, src: u32, len: u32) -> u32 {
    debug!(
        "emscripten::_emscripten_memcpy_big {}, {}, {}",
        dest, src, len
    );
    let dest_addr = emscripten_memory_pointer!(ctx.memory(0), dest) as *mut c_void;
    let src_addr = emscripten_memory_pointer!(ctx.memory(0), src) as *mut c_void;
    unsafe {
        memcpy(dest_addr, src_addr, len as size_t);
    }
    dest
}

/// emscripten: _emscripten_get_heap_size
pub fn _emscripten_get_heap_size(ctx: &mut Ctx) -> u32 {
    debug!("emscripten::_emscripten_get_heap_size",);
    ctx.memory(0).size().bytes().0 as u32
}

// From emscripten implementation
fn align_up(mut val: usize, multiple: usize) -> usize {
    if val % multiple > 0 {
        val += multiple - val % multiple;
    }
    val
}

/// emscripten: _emscripten_resize_heap
/// Note: this function only allows growing the size of heap
pub fn _emscripten_resize_heap(ctx: &mut Ctx, requested_size: u32) -> u32 {
    debug!("emscripten::_emscripten_resize_heap {}", requested_size);
    let current_memory_pages = ctx.memory(0).size();
    let current_memory = current_memory_pages.bytes().0 as u32;

    // implementation from emscripten
    let mut new_size = usize::max(current_memory as usize, WASM_MIN_PAGES * WASM_PAGE_SIZE);
    while new_size < requested_size as usize {
        if new_size <= 0x2000_0000 {
            new_size = align_up(new_size * 2, WASM_PAGE_SIZE);
        } else {
            new_size = usize::min(
                align_up((3 * new_size + 0x8000_0000) / 4, WASM_PAGE_SIZE),
                WASM_PAGE_SIZE * WASM_MAX_PAGES,
            );
        }
    }

    let amount_to_grow = (new_size - current_memory as usize) / WASM_PAGE_SIZE;
    if let Ok(_pages_allocated) = ctx.memory(0).grow(Pages(amount_to_grow as u32)) {
        debug!("{} pages allocated", _pages_allocated.0);
        1
    } else {
        0
    }
}

/// emscripten: getTotalMemory
pub fn get_total_memory(_ctx: &mut Ctx) -> u32 {
    debug!("emscripten::get_total_memory");
    // instance.memories[0].current_pages()
    // TODO: Fix implementation
    _ctx.memory(0).size().bytes().0 as u32
}

/// emscripten: enlargeMemory
pub fn enlarge_memory(_ctx: &mut Ctx) -> u32 {
    debug!("emscripten::enlarge_memory");
    // instance.memories[0].grow(100);
    // TODO: Fix implementation
    0
}

/// emscripten: abortOnCannotGrowMemory
pub fn abort_on_cannot_grow_memory(ctx: &mut Ctx, _requested_size: u32) -> u32 {
    debug!(
        "emscripten::abort_on_cannot_grow_memory {}",
        _requested_size
    );
    abort_with_message(ctx, "Cannot enlarge memory arrays!");
    0
}

/// emscripten: abortOnCannotGrowMemory
pub fn abort_on_cannot_grow_memory_old(ctx: &mut Ctx) -> u32 {
    debug!("emscripten::abort_on_cannot_grow_memory");
    abort_with_message(ctx, "Cannot enlarge memory arrays!");
    0
}

/// emscripten: ___map_file
pub fn ___map_file(_ctx: &mut Ctx, _one: u32, _two: u32) -> c_int {
    debug!("emscripten::___map_file");
    // NOTE: TODO: Em returns -1 here as well. May need to implement properly
    -1
}
