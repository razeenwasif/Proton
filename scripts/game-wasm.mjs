// Build Game mode's simulation as WebAssembly into ui/game-runtime.wasm, where
// the window loads it from. Run by the tests' pre-steps and before the app is
// bundled, so the wasm is never older than the Rust it is built from — it is
// not committed for the same reason.
//
//   node scripts/game-wasm.mjs
import { execFileSync } from "node:child_process";
import { copyFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const run = (args) => execFileSync("cargo", args, { cwd: root, stdio: ["ignore", "pipe", "inherit"] }).toString();

run(["build", "--quiet", "-p", "proton-game-runtime", "--target", "wasm32-unknown-unknown", "--release"]);
// Wherever cargo builds to: CARGO_TARGET_DIR, a target-dir in cargo's config,
// or the workspace's own target/.
const target = JSON.parse(run(["metadata", "--format-version", "1", "--no-deps"])).target_directory;
const built = join(target, "wasm32-unknown-unknown", "release", "proton_game_runtime.wasm");
const into = join(root, "ui", "game-runtime.wasm");
copyFileSync(built, into);
console.log(`game-runtime.wasm: ${statSync(into).size} bytes`);
