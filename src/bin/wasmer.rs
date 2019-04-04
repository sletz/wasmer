extern crate structopt;

use std::env;
use std::fs::{read_to_string, File};
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;

use hashbrown::HashMap;
use structopt::StructOpt;

use wasmer::webassembly::InstanceABI;
use wasmer::*;
use wasmer_runtime::cache::{Cache as BaseCache, FileSystemCache, WasmHash, WASMER_VERSION_HASH};
use wasmer_runtime_core::{self, backend::CompilerConfig};
#[cfg(feature = "wasi")]
use wasmer_wasi;

#[derive(Debug, StructOpt)]
#[structopt(name = "wasmer", about = "Wasm execution runtime.")]
/// The options for the wasmer Command Line Interface
enum CLIOptions {
    /// Run a WebAssembly file. Formats accepted: wasm, wast
    #[structopt(name = "run")]
    Run(Run),

    /// Wasmer cache
    #[structopt(name = "cache")]
    Cache(Cache),

    /// Validate a Web Assembly binary
    #[structopt(name = "validate")]
    Validate(Validate),

    /// Update wasmer to the latest version
    #[structopt(name = "self-update")]
    SelfUpdate,
}

#[derive(Debug, StructOpt)]
struct Run {
    // Disable the cache
    #[structopt(long = "disable-cache")]
    disable_cache: bool,

    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,

    /// Application arguments
    #[structopt(name = "--", raw(multiple = "true"))]
    args: Vec<String>,

    /// Emscripten symbol map
    #[structopt(long = "em-symbol-map", parse(from_os_str))]
    em_symbol_map: Option<PathBuf>,
}

#[derive(Debug, StructOpt)]
enum Cache {
    /// Clear the cache
    #[structopt(name = "clean")]
    Clean,

    /// Display the location of the cache
    #[structopt(name = "dir")]
    Dir,
}

#[derive(Debug, StructOpt)]
struct Validate {
    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,
}

/// Read the contents of a file
fn read_file_contents(path: &PathBuf) -> Result<Vec<u8>, io::Error> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut file = File::open(path)?;
    file.read_to_end(&mut buffer)?;
    // We force to close the file
    drop(file);
    Ok(buffer)
}

fn get_cache_dir() -> PathBuf {
    match env::var("WASMER_CACHE_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => {
            // We use a temporal directory for saving cache files
            let mut temp_dir = env::temp_dir();
            temp_dir.push("wasmer");
            temp_dir.push(WASMER_VERSION_HASH);
            temp_dir
        }
    }
}

/// Execute a wasm/wat file
fn execute_wasm(options: &Run) -> Result<(), String> {
    // force disable caching on windows
    #[cfg(target_os = "windows")]
    let disable_cache = true;
    #[cfg(not(target_os = "windows"))]
    let disable_cache = options.disable_cache;

    let wasm_path = &options.path;

    let mut wasm_binary: Vec<u8> = read_file_contents(wasm_path).map_err(|err| {
        format!(
            "Can't read the file {}: {}",
            wasm_path.as_os_str().to_string_lossy(),
            err
        )
    })?;

    let em_symbol_map = if let Some(em_symbol_map_path) = options.em_symbol_map.clone() {
        let em_symbol_map_content: String = read_to_string(&em_symbol_map_path)
            .map_err(|err| {
                format!(
                    "Can't read symbol map file {}: {}",
                    em_symbol_map_path.as_os_str().to_string_lossy(),
                    err,
                )
            })?
            .to_owned();
        let mut em_symbol_map = HashMap::new();
        for line in em_symbol_map_content.lines() {
            let mut split = line.split(':');
            let num_str = if let Some(ns) = split.next() {
                ns
            } else {
                return Err(format!(
                    "Can't parse symbol map (expected each entry to be of the form: `0:func_name`)"
                ));
            };
            let num: u32 = num_str.parse::<u32>().map_err(|err| {
                format!(
                    "Failed to parse {} as a number in symbol map: {}",
                    num_str, err
                )
            })?;
            let name_str: String = if let Some(name_str) = split.next() {
                name_str
            } else {
                return Err(format!(
                    "Can't parse symbol map (expected each entry to be of the form: `0:func_name`)"
                ));
            }
            .to_owned();

            em_symbol_map.insert(num, name_str);
        }
        Some(em_symbol_map)
    } else {
        None
    };

    if !utils::is_wasm_binary(&wasm_binary) {
        wasm_binary = wabt::wat2wasm(wasm_binary)
            .map_err(|e| format!("Can't convert from wast to wasm: {:?}", e))?;
    }

    let module = if !disable_cache {
        // If we have cache enabled

        // We generate a hash for the given binary, so we can use it as key
        // for the Filesystem cache
        let hash = WasmHash::generate(&wasm_binary);

        let wasmer_cache_dir = get_cache_dir();

        // We create a new cache instance.
        // It could be possible to use any other kinds of caching, as long as they
        // implement the Cache trait (with save and load functions)
        let mut cache = unsafe {
            FileSystemCache::new(wasmer_cache_dir).map_err(|e| format!("Cache error: {:?}", e))?
        };

        // cache.load will return the Module if it's able to deserialize it properly, and an error if:
        // * The file is not found
        // * The file exists, but it's corrupted or can't be converted to a module
        let module = match cache.load(hash) {
            Ok(module) => {
                // We are able to load the module from cache
                module
            }
            Err(_) => {
                let module = webassembly::compile_with_config(
                    &wasm_binary[..],
                    CompilerConfig {
                        symbol_map: em_symbol_map,
                    },
                )
                .map_err(|e| format!("Can't compile module: {:?}", e))?;
                // We try to save the module into a cache file
                cache.store(hash, module.clone()).unwrap_or_default();

                module
            }
        };
        module
    } else {
        webassembly::compile_with_config(
            &wasm_binary[..],
            CompilerConfig {
                symbol_map: em_symbol_map,
            },
        )
        .map_err(|e| format!("Can't compile module: {:?}", e))?
    };

    // TODO: refactor this
    #[cfg(not(feature = "wasi"))]
    let (abi, import_object, _em_globals) = if wasmer_emscripten::is_emscripten_module(&module) {
        let mut emscripten_globals = wasmer_emscripten::EmscriptenGlobals::new(&module);
        (
            InstanceABI::Emscripten,
            wasmer_emscripten::generate_emscripten_env(&mut emscripten_globals),
            Some(emscripten_globals), // TODO Em Globals is here to extend, lifetime, find better solution
        )
    } else {
        (
            InstanceABI::None,
            wasmer_runtime_core::import::ImportObject::new(),
            None,
        )
    };

    #[cfg(feature = "wasi")]
    let (abi, import_object) = if wasmer_wasi::is_wasi_module(&module) {
        (
            InstanceABI::WASI,
            wasmer_wasi::generate_import_object(
                [options.path.to_str().unwrap().to_owned()]
                    .iter()
                    .chain(options.args.iter())
                    .cloned()
                    .map(|arg| arg.into_bytes())
                    .collect(),
                env::vars()
                    .map(|(k, v)| format!("{}={}", k, v).into_bytes())
                    .collect(),
            ),
        )
    } else {
        (
            InstanceABI::None,
            wasmer_runtime_core::import::ImportObject::new(),
        )
    };

    let mut instance = module
        .instantiate(&import_object)
        .map_err(|e| format!("Can't instantiate module: {:?}", e))?;

    webassembly::run_instance(
        &module,
        &mut instance,
        abi,
        options.path.to_str().unwrap(),
        options.args.iter().map(|arg| arg.as_str()).collect(),
    )
    .map_err(|e| format!("{:?}", e))?;

    Ok(())
}

fn run(options: Run) {
    match execute_wasm(&options) {
        Ok(()) => {}
        Err(message) => {
            eprintln!("{:?}", message);
            exit(1);
        }
    }
}

fn validate_wasm(validate: Validate) -> Result<(), String> {
    let wasm_path = validate.path;
    let wasm_path_as_str = wasm_path.to_str().unwrap();

    let wasm_binary: Vec<u8> = read_file_contents(&wasm_path).map_err(|err| {
        format!(
            "Can't read the file {}: {}",
            wasm_path.as_os_str().to_string_lossy(),
            err
        )
    })?;

    if !utils::is_wasm_binary(&wasm_binary) {
        return Err(format!(
            "Cannot recognize \"{}\" as a WASM binary",
            wasm_path_as_str,
        ));
    }

    wasmer_runtime_core::validate_and_report_errors(&wasm_binary)
        .map_err(|err| format!("Validation failed: {}", err))?;

    Ok(())
}

/// Runs logic for the `validate` subcommand
fn validate(validate: Validate) {
    match validate_wasm(validate) {
        Err(message) => {
            eprintln!("Error: {}", message);
            exit(-1);
        }
        _ => (),
    }
}

fn main() {
    let options = CLIOptions::from_args();
    match options {
        CLIOptions::Run(options) => run(options),
        #[cfg(not(target_os = "windows"))]
        CLIOptions::SelfUpdate => update::self_update(),
        #[cfg(target_os = "windows")]
        CLIOptions::SelfUpdate => {
            println!("Self update is not supported on Windows. Use install instructions on the Wasmer homepage: https://wasmer.io");
        }
        #[cfg(not(target_os = "windows"))]
        CLIOptions::Cache(cache) => match cache {
            Cache::Clean => {
                use std::fs;
                let cache_dir = get_cache_dir();
                if cache_dir.exists() {
                    fs::remove_dir_all(cache_dir.clone()).expect("Can't remove cache dir");
                }
                fs::create_dir_all(cache_dir.clone()).expect("Can't create cache dir");
            }
            Cache::Dir => {
                println!("{}", get_cache_dir().to_string_lossy());
            }
        },
        CLIOptions::Validate(validate_options) => {
            validate(validate_options);
        }
        #[cfg(target_os = "windows")]
        CLIOptions::Cache(_) => {
            println!("Caching is disabled for Windows.");
        }
    }
}
