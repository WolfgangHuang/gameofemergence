// Builds the Rust engine and copies it to where Vite picks it up.
// `--test` runs the engine's test suite natively instead, which needs no C toolchain
// either, see engine/.cargo/config.toml. It used to run under WASI in Node, whose
// process dies of a segmentation fault once tests generate a large field, whatever the
// flags. The engine as plain WASM does not, see scripts/check-hat-glider.mjs.
import { execFileSync } from 'node:child_process';
import { copyFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { delimiter, join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const engine = join(root, 'engine');
const env = { ...process.env, PATH: join(homedir(), '.cargo', 'bin') + delimiter + process.env.PATH };
const cargo = (...args) => execFileSync('cargo', args, { cwd: engine, env, stdio: 'inherit' });

if (process.argv.includes('--test')) {
  const target = 'x86_64-unknown-linux-musl';
  execFileSync('rustup', ['target', 'add', target], { env, stdio: 'inherit' });
  cargo('test', '--release', '--target', target, '--lib');
} else {
  cargo('build', '--release', '--target', 'wasm32-unknown-unknown', '--lib');
  copyFileSync(
    join(engine, 'target/wasm32-unknown-unknown/release/engine.wasm'),
    join(root, 'src/sim/engine.wasm'),
  );
}
