#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::*;

#[cfg(windows)]
pub use self::windows::*;

use crate::{allocate_on_stack, EmscriptenData};
use std::os::raw::c_int;
use wasmer_runtime_core::vm::Ctx;

pub fn _getaddrinfo(_ctx: &mut Ctx, _one: i32, _two: i32, _three: i32, _four: i32) -> i32 {
    debug!("emscripten::_getaddrinfo");
    -1
}

pub fn call_malloc(ctx: &mut Ctx, size: u32) -> u32 {
    get_emscripten_data(ctx).malloc.call(size).unwrap()
}

pub fn call_memalign(ctx: &mut Ctx, alignment: u32, size: u32) -> u32 {
    if let Some(memalign) = &get_emscripten_data(ctx).memalign {
        memalign.call(alignment, size).unwrap()
    } else {
        panic!("Memalign is set to None");
    }
}

pub fn call_memset(ctx: &mut Ctx, pointer: u32, value: u32, size: u32) -> u32 {
    get_emscripten_data(ctx)
        .memset
        .call(pointer, value, size)
        .unwrap()
}

pub(crate) fn get_emscripten_data(ctx: &mut Ctx) -> &mut EmscriptenData {
    unsafe { &mut *(ctx.data as *mut EmscriptenData) }
}

pub fn _getpagesize(_ctx: &mut Ctx) -> u32 {
    debug!("emscripten::_getpagesize");
    16384
}

pub fn _times(ctx: &mut Ctx, buffer: u32) -> u32 {
    if buffer != 0 {
        call_memset(ctx, buffer, 0, 16);
    }
    0
}

#[allow(clippy::cast_ptr_alignment)]
pub fn ___build_environment(ctx: &mut Ctx, environ: c_int) {
    debug!("emscripten::___build_environment {}", environ);
    const MAX_ENV_VALUES: u32 = 64;
    const TOTAL_ENV_SIZE: u32 = 1024;
    let environment = emscripten_memory_pointer!(ctx.memory(0), environ) as *mut c_int;
    let (mut pool_offset, env_ptr, mut pool_ptr) = unsafe {
        let (pool_offset, _pool_slice): (u32, &mut [u8]) =
            allocate_on_stack(ctx, TOTAL_ENV_SIZE as u32);
        let (env_offset, _env_slice): (u32, &mut [u8]) =
            allocate_on_stack(ctx, (MAX_ENV_VALUES * 4) as u32);
        let env_ptr = emscripten_memory_pointer!(ctx.memory(0), env_offset) as *mut c_int;
        let pool_ptr = emscripten_memory_pointer!(ctx.memory(0), pool_offset) as *mut u8;
        *env_ptr = pool_offset as i32;
        *environment = env_offset as i32;

        (pool_offset, env_ptr, pool_ptr)
    };

    // *env_ptr = 0;
    let default_vars = vec![
        ["USER", "web_user"],
        ["LOGNAME", "web_user"],
        ["PATH", "/"],
        ["PWD", "/"],
        ["HOME", "/home/web_user"],
        ["LANG", "C.UTF-8"],
        ["_", "thisProgram"],
    ];
    let mut strings = vec![];
    let mut total_size = 0;
    for [key, val] in &default_vars {
        let line = key.to_string() + "=" + val;
        total_size += line.len();
        strings.push(line);
    }
    if total_size as u32 > TOTAL_ENV_SIZE {
        panic!("Environment size exceeded TOTAL_ENV_SIZE!");
    }
    unsafe {
        for (i, s) in strings.iter().enumerate() {
            for (j, c) in s.chars().enumerate() {
                debug_assert!(c < u8::max_value() as char);
                *pool_ptr.add(j) = c as u8;
            }
            *env_ptr.add(i * 4) = pool_offset as i32;
            pool_offset += s.len() as u32 + 1;
            pool_ptr = pool_ptr.add(s.len() + 1);
        }
        *env_ptr.add(strings.len() * 4) = 0;
    }
}

pub fn ___assert_fail(_ctx: &mut Ctx, _a: c_int, _b: c_int, _c: c_int, _d: c_int) {
    debug!("emscripten::___assert_fail {} {} {} {}", _a, _b, _c, _d);
    // TODO: Implement like emscripten expects regarding memory/page size
    // TODO raise an error
}
