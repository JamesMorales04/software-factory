import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
};
const port = Number(process.env.PORT || 4173);

createServer(async (request, response) => {
  const raw = (request.url || "/").split("?")[0];
  const rel = raw === "/" ? "index.html" : raw.replace(/^\/+/, "");
  const file = join(root, rel);
  if (!file.startsWith(root)) {
    response.writeHead(403).end();
    return;
  }
  try {
    const info = await stat(file);
    if (!info.isFile()) throw new Error("not a file");
    response.writeHead(200, { "Content-Type": types[extname(file)] || "application/octet-stream" });
    response.end(await readFile(file));
  } catch {
    response.writeHead(404).end();
  }
}).listen(port, "127.0.0.1", () => {
  console.log(`vlog fixture listening on http://127.0.0.1:${port}/`);
});
