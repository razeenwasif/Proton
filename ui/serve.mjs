// Static server for the Proton design preview. No dependencies.
//   node ui/serve.mjs   →  http://localhost:5178
import { createReadStream, statSync } from "node:fs";
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
};

createServer((req, res) => {
  let path = decodeURIComponent(req.url.split("?")[0]);
  if (path === "/") path = "/ui/index.html";

  // Keep requests inside the project root.
  const file = join(ROOT, normalize(path).replace(/^(\.\.[/\\])+/, ""));
  if (!file.startsWith(ROOT)) {
    res.writeHead(403).end("forbidden");
    return;
  }

  let stat;
  try {
    stat = statSync(file);
  } catch {
    res.writeHead(404).end("not found");
    return;
  }
  if (!stat.isFile()) {
    res.writeHead(404).end("not found");
    return;
  }

  res.writeHead(200, {
    "Content-Type": TYPES[extname(file)] ?? "application/octet-stream",
    "Content-Length": stat.size,
    "Cache-Control": "no-cache",
  });
  createReadStream(file).pipe(res);
}).listen(PORT, () => console.log(`Proton preview → http://localhost:${PORT}`));
