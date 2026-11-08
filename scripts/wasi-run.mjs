// Cargo test runner for the wasm32-wasip1 target, so that tests need no C toolchain.
import { readFileSync } from 'node:fs';
import { WASI } from 'node:wasi';

const [file, ...args] = process.argv.slice(2);
const wasi = new WASI({ version: 'preview1', args: [file, ...args], env: process.env });
const { instance } = await WebAssembly.instantiate(readFileSync(file), wasi.getImportObject());
process.exitCode = wasi.start(instance);
