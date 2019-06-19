// use super::varargs::VarArgs;
use wasmer_runtime_core::vm::Ctx;

#[allow(clippy::cast_ptr_alignment)]
pub fn _sigemptyset(ctx: &mut Ctx, set: u32) -> i32 {
    debug!("emscripten::_sigemptyset");
    let set_addr = emscripten_memory_pointer!(ctx.memory(0), set) as *mut u32;
    unsafe {
        *set_addr = 0;
    }
    0
}

pub fn _sigaction(_ctx: &mut Ctx, _signum: u32, _act: u32, _oldact: u32) -> i32 {
    debug!("emscripten::_sigaction {}, {}, {}", _signum, _act, _oldact);
    0
}

pub fn _siginterrupt(_ctx: &mut Ctx, _a: u32, _b: u32) -> i32 {
    debug!("emscripten::_siginterrupt {}, {}", _a, _b);
    0
}

#[allow(clippy::cast_ptr_alignment)]
pub fn _sigaddset(ctx: &mut Ctx, set: u32, signum: u32) -> i32 {
    debug!("emscripten::_sigaddset {}, {}", set, signum);
    let set_addr = emscripten_memory_pointer!(ctx.memory(0), set) as *mut u32;
    unsafe {
        *set_addr |= 1 << (signum - 1);
    }
    0
}

pub fn _sigsuspend(_ctx: &mut Ctx, _one: i32) -> i32 {
    debug!("emscripten::_sigsuspend");
    -1
}

pub fn _sigprocmask(_ctx: &mut Ctx, _one: i32, _two: i32, _three: i32) -> i32 {
    debug!("emscripten::_sigprocmask");
    0
}

pub fn _signal(_ctx: &mut Ctx, _sig: u32, _two: i32) -> i32 {
    debug!("emscripten::_signal ({})", _sig);
    0
}
