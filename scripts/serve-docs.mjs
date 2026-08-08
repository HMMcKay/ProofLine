import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, extname, join, normalize, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const root = resolve(scriptDirectory, "..", "docs");
const requestedPort = Number.parseInt(process.argv[2] ?? process.env.PROOFLINE_DOCS_PORT ?? "4173", 10);
const port = Number.isInteger(requestedPort) && requestedPort > 0 && requestedPort < 65536 ? requestedPort : 4173;
const mimeTypes = new Map([
  [".css", "text/css; charset=utf-8"],
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".png", "image/png"],
  [".svg", "image/svg+xml"],
  [".txt", "text/plain; charset=utf-8"],
]);

function resolveRequestPath(url) {
  const pathname = decodeURIComponent(new URL(url, "http://localhost").pathname);
  const candidate = normalize(resolve(root, `.${pathname}`));
  const pathWithinRoot = relative(root, candidate);
  if (pathWithinRoot.startsWith("..") || pathWithinRoot.includes(`..${process.platform === "win32" ? "\\" : "/"}`)) return null;
  if (existsSync(candidate) && statSync(candidate).isDirectory()) return join(candidate, "index.html");
  return candidate;
}

const server = createServer((request, response) => {
  if (!request.url || !["GET", "HEAD"].includes(request.method ?? "")) {
    response.writeHead(405, { Allow: "GET, HEAD" }).end();
    return;
  }
  let target;
  try { target = resolveRequestPath(request.url); }
  catch { target = null; }
  if (!target || !existsSync(target) || !statSync(target).isFile()) {
    const notFound = join(root, "404.html");
    response.writeHead(404, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" });
    if (request.method === "HEAD") response.end();
    else createReadStream(notFound).pipe(response);
    return;
  }
  response.writeHead(200, {
    "Content-Type": mimeTypes.get(extname(target).toLowerCase()) ?? "application/octet-stream",
    "Cache-Control": "no-store",
    "X-Content-Type-Options": "nosniff",
  });
  if (request.method === "HEAD") response.end();
  else createReadStream(target).pipe(response);
});

server.listen(port, "127.0.0.1", () => {
  console.log(`ProofLine documentation: http://127.0.0.1:${port}/`);
  console.log("Press Ctrl+C to stop.");
});

