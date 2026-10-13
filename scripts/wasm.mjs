// Build the engines the window runs as WebAssembly into ui/, where it loads
// them from: Game mode's simulation (game-runtime.wasm) and Canvas mode's
// pictures (raster-runtime.wasm). Run by the tests' pre-steps and before the
// app is bundled, so neither is ever older than the Rust it is built from —
// they are not committed for the same reason. One cargo call builds both.
//
//   node scripts/wasm.mjs
import { execFileSync } from "node:child_process";
import { copyFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const run = (args) => execFileSync("cargo", args, { cwd: root, stdio: ["ignore", "pipe", "inherit"] }).toString();
const RUNTIMES = [
  ["proton-game-runtime", "proton_game_runtime.wasm", "game-runtime.wasm"],
  ["proton-raster-runtime", "proton_raster_runtime.wasm", "raster-runtime.wasm"],
];

run(["build", "--quiet", ...RUNTIMES.flatMap(([crate]) => ["-p", crate]), "--target", "wasm32-unknown-unknown", "--release"]);
// Wherever cargo builds to: CARGO_TARGET_DIR, a target-dir in cargo's config,
// or the workspace's own target/.
const target = JSON.parse(run(["metadata", "--format-version", "1", "--no-deps"])).target_directory;
for (const [, built, into] of RUNTIMES) {
  const to = join(root, "ui", into);
  copyFileSync(join(target, "wasm32-unknown-unknown", "release", built), to);
  console.log(`${into}: ${statSync(to).size} bytes`);
}
