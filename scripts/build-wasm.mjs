// Builds the Rust engine and copies it to where Vite picks it up.
// `--test` runs the engine's test suite under WASI in Node instead.
import { execFileSync } from 'node:child_process';
import { copyFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { delimiter, join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const engine = join(root, 'engine');
const env = { ...process.env, PATH: join(homedir(), '.cargo', 'bin') + delimiter + process.env.PATH };
const cargo = (...args) => execFileSync('cargo', args, { cwd: engine, env, stdio: 'inherit' });

if (process.argv.includes('--test')) {
  execFileSync('rustup', ['target', 'add', 'wasm32-wasip1'], { env, stdio: 'inherit' });
  env.CARGO_TARGET_WASM32_WASIP1_RUNNER = `node --no-warnings ${join(root, 'scripts/wasi-run.mjs')}`;
  cargo('test', '--release', '--target', 'wasm32-wasip1', '--lib');
} else {
  cargo('build', '--release', '--target', 'wasm32-unknown-unknown', '--lib');
  copyFileSync(
    join(engine, 'target/wasm32-unknown-unknown/release/engine.wasm'),
    join(root, 'src/sim/engine.wasm'),
  );
}
