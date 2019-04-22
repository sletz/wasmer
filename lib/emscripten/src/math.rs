use wasmer_runtime_core::vm::Ctx;

/// emscripten: _llvm_log10_f64
pub fn _llvm_log10_f64(_ctx: &mut Ctx, value: f64) -> f64 {
    debug!("emscripten::_llvm_log10_f64");
    value.log10()
}

/// emscripten: _llvm_log2_f64
pub fn _llvm_log2_f64(_ctx: &mut Ctx, value: f64) -> f64 {
    debug!("emscripten::_llvm_log2_f64");
    value.log2()
}

/// emscripten: _llvm_sin_f64
pub fn _llvm_sin_f64(_ctx: &mut Ctx, value: f64) -> f64 {
    debug!("emscripten::_llvm_sin_f64");
    value.sin()
}

/// emscripten: _llvm_cos_f64
pub fn _llvm_cos_f64(_ctx: &mut Ctx, value: f64) -> f64 {
    debug!("emscripten::_llvm_cos_f64");
    value.cos()
}

pub fn _llvm_log10_f32(_ctx: &mut Ctx, _value: f64) -> f64 {
    debug!("emscripten::_llvm_log10_f32");
    -1.0
}

pub fn _llvm_log2_f32(_ctx: &mut Ctx, _value: f64) -> f64 {
    debug!("emscripten::_llvm_log10_f32");
    -1.0
}

pub fn _llvm_exp2_f32(_ctx: &mut Ctx, value: f32) -> f32 {
    debug!("emscripten::_llvm_exp2_f32");
    2f32.powf(value)
}

pub fn _llvm_exp2_f64(_ctx: &mut Ctx, value: f64) -> f64 {
    debug!("emscripten::_llvm_exp2_f64");
    2f64.powf(value)
}

pub fn _emscripten_random(_ctx: &mut Ctx) -> f64 {
    debug!("emscripten::_emscripten_random");
    -1.0
}

// emscripten: asm2wasm.f64-rem
pub fn f64_rem(_ctx: &mut Ctx, x: f64, y: f64) -> f64 {
    debug!("emscripten::f64-rem");
    x % y
}

// emscripten: global.Math pow
pub fn pow(_ctx: &mut Ctx, x: f64, y: f64) -> f64 {
    x.powf(y)
}

// emscripten: global.Math exp
pub fn exp(_ctx: &mut Ctx, value: f64) -> f64 {
    value.exp()
}

// emscripten: global.Math log
pub fn log(_ctx: &mut Ctx, value: f64) -> f64 {
    value.ln()
}

// emscripten: asm2wasm.f64-to-int
pub fn f64_to_int(_ctx: &mut Ctx, value: f64) -> i32 {
    debug!("emscripten::f64_to_int {}", value);
    value as i32
}
