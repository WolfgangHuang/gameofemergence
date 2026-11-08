// Builds the search as one static binary, which also runs on other Linux machines.
// It needs no C toolchain, see engine/.cargo/config.toml.
import { execFileSync } from 'node:child_process';
import { homedir } from 'node:os';
import { delimiter, join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const engine = join(root, 'engine');
const env = { ...process.env, PATH: join(homedir(), '.cargo', 'bin') + delimiter + process.env.PATH };
const target = 'x86_64-unknown-linux-musl';
execFileSync('rustup', ['target', 'add', target], { env, stdio: 'inherit' });
execFileSync('cargo', ['build', '--release', '--target', target, '--bin', 'search'], { cwd: engine, env, stdio: 'inherit' });
console.log(`built ${join(engine, 'target', target, 'release', 'search')}`);
