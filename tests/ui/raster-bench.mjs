// The Canvas brush bench (raster-bench-run.js) in node, from the built
// engine: `node scripts/wasm.mjs && node tests/ui/raster-bench.mjs`.
import {readFileSync} from 'node:fs';
import {performance} from 'node:perf_hooks';
import {bench} from './raster-bench-run.js';

const over = await bench(readFileSync(new URL('../../ui/raster-runtime.wasm', import.meta.url)), () => performance.now(), (line) => console.log(line));
process.exitCode = over ? 1 : 0;
