#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

#[macro_use]
extern crate wasmer_runtime_core;
// extern crate wasmer_emscripten;

#[macro_use]
pub mod update;
pub mod utils;
pub mod webassembly;
