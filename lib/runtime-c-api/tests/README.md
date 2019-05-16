# Faust wasmer tools
Several programs are available to test the wasmer based compilation chain.

## faustwasmer

The **faustwasmer** opens a DSP wasm module and run it with the wasmer machinery. The DSP module is JIT compiled and opened as a JACK client.

So for instance:
faust -lang noise.dsp -o noise.wasm
faustwasmer noise.wasm

## faustbench-wasmer

The **faustbench-wasmer** allows to benchmark a DSP wasm module. 

So for instance:
faust -lang noise.dsp -o noise.wasm
austbench-wasmer noise.wasm
