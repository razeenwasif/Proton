// Static server for the Proton design preview. No dependencies.
//   node ui/serve.mjs   →  http://localhost:5178
import { createReadStream, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = Number(process.env.PORT) || 5178;

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".webp": "image/webp",
  ".ico": "image/x-icon",
  ".mp4": "video/mp4",
  ".woff2": "font/woff2",
  ".woff": "font/woff",
  ".wasm": "application/wasm",
};

/** The app's own Content-Security-Policy, for a page asked for with `?csp=1`:
 *  what the shell's window enforces, so a test can see it hold. */
const CSP = (() => {
  try {
    const conf = JSON.parse(readFileSync(join(ROOT, "crates", "proton-shell", "tauri.conf.json"), "utf8"));
    return conf.app?.security?.csp ?? null;
  } catch {
    return null;
  }
})();

/** The first candidate that is a real file, or null. */
function resolve(path) {
  // `ui/` is the web root, so `styles.css` resolves the way it does in the
  // shell, where `frontendDist` points at that directory. Falling back to the
  // project root afterwards is what lets the preview's stand-in read real
  // source files — `openBuffer` fetches `crates/…`, which is not under `ui/`.
  for (const base of [join(ROOT, "ui"), ROOT]) {
    const file = join(base, path);
    // Both bases are inside ROOT, so one check covers the traversal case.
    if (!file.startsWith(ROOT)) continue;
    try {
      if (statSync(file).isFile()) return file;
    } catch {
      /* try the next base */
    }
  }
  return null;
}

createServer((req, res) => {
  let path = decodeURIComponent(req.url.split("?")[0]);
  if (path === "/") path = "/index.html";
  path = normalize(path).replace(/^(\.\.[/\\])+/, "");

  const file = resolve(path);
  if (!file) {
    res.writeHead(404).end("not found");
    return;
  }
  const stat = statSync(file);

  const strict = /[?&]csp=1\b/.test(req.url) && extname(file) === ".html" && CSP;
  res.writeHead(200, {
    "Content-Type": TYPES[extname(file)] ?? "application/octet-stream",
    "Content-Length": stat.size,
    "Cache-Control": "no-cache",
    ...(strict ? { "Content-Security-Policy": CSP } : {}),
  });
  createReadStream(file).pipe(res);
}).listen(PORT, '127.0.0.1', () => console.log(`Proton preview → http://localhost:${PORT}`));
