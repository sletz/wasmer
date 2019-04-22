<p align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="400" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/logo.png" alt="Wasmer logo">
  </a>
</p>

<p align="center">
  <a href="https://circleci.com/gh/wasmerio/wasmer/">
    <img src="https://img.shields.io/circleci/project/github/wasmerio/wasmer/master.svg" alt="Build Status">
  </a>
  <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
    <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg" alt="License">
  </a>
  <a href="https://spectrum.chat/wasmer">
    <img src="https://withspectrum.github.io/badge/badge.svg" alt="Join the Wasmer Community">
  </a>
</p>

## Introduction

[Wasmer](https://wasmer.io/) is a standalone JIT WebAssembly runtime, aiming to be fully compatible with [WASI](https://hacks.mozilla.org/2019/03/standardizing-wasi-a-webassembly-system-interface/) and [Emscripten](https://emscripten.org/).

Install Wasmer with:

```sh
curl https://get.wasmer.io -sSfL | sh
```

Wasmer runtime can also be embedded in different languages, so you can use WebAssembly anywhere ✨:
* [**Rust**](https://github.com/wasmerio/wasmer-rust-example)
* [**C/C++**](https://github.com/wasmerio/wasmer-c-api)
* [**PHP**](https://github.com/wasmerio/php-ext-wasm)
* [**Python**](https://github.com/wasmerio/python-ext-wasm)

### Usage

Wasmer can execute both the standard binary format (`.wasm`) and the text
format defined by the WebAssembly reference interpreter (`.wat`).

Once installed, you will be able to run any WebAssembly files (_including Lua, PHP, SQLite and nginx!_):

```sh
# Run Lua
wasmer run examples/lua.wasm

# Run PHP
wasmer run examples/php.wasm

# Run SQLite
wasmer run examples/sqlite.wasm

# Run nginx
wasmer run examples/nginx/nginx.wasm -- -p examples/nginx -c nginx.conf
```

## Code Structure

Wasmer is structured into different directories:

- [`src`](./src): code related to the Wasmer executable itself
- [`lib`](./lib): modularized libraries that Wasmer uses under the hood
- [`examples`](./examples): some useful examples to getting started with Wasmer

## Dependencies

Building Wasmer requires [rustup](https://rustup.rs/).

To build on Windows, download and run [`rustup-init.exe`](https://win.rustup.rs/)
then follow the onscreen instructions.

To build on other systems, run:

```sh
curl https://sh.rustup.rs -sSf | sh
```

### Other dependencies

Please select your operating system:

- [macOS](#macos)
- [Debian-based Linuxes](#debian-based-linuxes)
- [FreeBSD](#freebsd)
- [Microsoft Windows](#windows-msvc)

#### macOS

If you have [Homebrew](https://brew.sh/) installed:

```sh
brew install cmake
```

Or, in case you have [MacPorts](https://www.macports.org/install.php):

```sh
sudo port install cmake
```

#### Debian-based Linuxes

```sh
sudo apt install cmake
```

#### FreeBSD

```sh
pkg install cmake
```

#### Windows (MSVC)

Windows support is _highly experimental_. Only simple Wasm programs may be run, and no syscalls are allowed. This means
nginx and Lua do not work on Windows. See [this issue](https://github.com/wasmerio/wasmer/issues/176) regarding Emscripten syscall polyfills for Windows.

1. Install [Visual Studio](https://visualstudio.microsoft.com/thank-you-downloading-visual-studio/?sku=Community&rel=15)

2. Install [Rust for Windows](https://win.rustup.rs)

3. Install [Python for Windows](https://www.python.org/downloads/release/python-2714/). The Windows x86-64 MSI installer is fine.
   Make sure to enable "Add python.exe to Path" during installation.

4. Install [Git for Windows](https://git-scm.com/download/win). Allow it to add `git.exe` to your PATH (default
   settings for the installer are fine).

5. Install [CMake](https://cmake.org/download/). Ensure CMake is in your PATH.

6. Install [LLVM 7.0](https://prereleases.llvm.org/win-snapshots/LLVM-7.0.0-r336178-win64.exe)

## Building

Wasmer is built with [Cargo](https://crates.io/), the Rust package manager.

```sh
# checkout code
git clone https://github.com/wasmerio/wasmer.git
cd wasmer

# install tools
# make sure that `python` is accessible.
cargo install --path .
```

## Testing

Thanks to [spec tests](https://github.com/wasmerio/wasmer/tree/master/lib/spectests/spectests) we can ensure 100% compatibility with the WebAssembly spec test suite.

Tests can be run with:

```sh
make test
```

If you need to regenerate the Rust tests from the spec tests
you can run:

```sh
make spectests
```

You can also run integration tests with:

```sh
make integration-tests
```

## Benchmarking

Benchmarks can be run with:

```sh
cargo bench --all
```

## Roadmap

Wasmer is an open project guided by strong principles, aiming to be modular, flexible and fast. It is open to the community to help set its direction.

Below are some of the goals of this project (in order of priority):

- [x] It should be 100% compatible with the [WebAssembly spec tests](https://github.com/wasmerio/wasmer/tree/master/lib/spectests/spectests)
- [x] It should be fast _(partially achieved)_
- [ ] Support WASI _(in the works)_
- [ ] Support Emscripten calls _(in the works)_
- [ ] Support Rust ABI calls
- [ ] Support Go ABI calls

## Architecture

If you would like to know how Wasmer works under the hood, please see [ARCHITECTURE.md](./ARCHITECTURE.md).

## License

Wasmer is primarily distributed under the terms of the [MIT license](http://opensource.org/licenses/MIT) ([LICENSE](./LICENSE)).

[ATTRIBUTIONS](./ATTRIBUTIONS.md)
