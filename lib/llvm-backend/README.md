<p align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/logo.png" alt="Wasmer logo">
  </a>
</p>

<p align="center">
  <a href="https://dev.azure.com/wasmerio/wasmer/_build/latest?definitionId=3&branchName=master">
    <img src="https://img.shields.io/azure-devops/build/wasmerio/wasmer/3.svg?style=flat-square" alt="Build Status">
  </a>
  <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
    <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="License">
  </a>
  <a href="https://spectrum.chat/wasmer">
    <img src="https://withspectrum.github.io/badge/badge.svg" alt="Join the Wasmer Community">
  </a>
  <a href="https://crates.io/crates/wasmer-llvm-backend">
    <img src="https://img.shields.io/crates/d/wasmer-llvm-backend.svg?style=flat-square" alt="Number of downloads from crates.io">
  </a>
  <a href="https://docs.rs/wasmer-llvm-backend">
    <img src="https://docs.rs/wasmer-llvm-backend/badge.svg" alt="Read our API documentation">
  </a>
</p>

# Wasmer LLVM backend

Wasmer is a standalone JIT WebAssembly runtime, aiming to be fully
compatible with Emscripten, Rust and Go. [Learn
more](https://github.com/wasmerio/wasmer).

This crate represents the LLVM backend integration for Wasmer.

## Usage

### Usage in Wasmer Standalone

If you are using the `wasmer` CLI, you can specify the backend with:

```sh
wasmer run program.wasm --backend=llvm
```

### Usage in Wasmer Embedded

If you are using Wasmer Embedded, you can specify
the LLVM backend to the [`compile_with` function](https://docs.rs/wasmer-runtime-core/*/wasmer_runtime_core/fn.compile_with.html):

```rust
use wasmer_llvm_backend::LLVMCompiler;

// ...
let module = wasmer_runtime_core::compile_with(&wasm_binary[..], &LLVMCompiler::new());
```
