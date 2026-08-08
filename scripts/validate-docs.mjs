import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, extname, join, normalize, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "..");
const documentationRoot = join(repositoryRoot, "docs");
const failures = [];
const htmlFiles = [];

function walk(directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) walk(path);
    else if (entry.isFile() && extname(entry.name).toLowerCase() === ".html") htmlFiles.push(path);
  }
}

function record(file, message) {
  failures.push(`${relative(repositoryRoot, file)}: ${message}`);
}

function localTargetExists(file, reference) {
  const withoutFragment = reference.split("#", 1)[0].split("?", 1)[0];
  if (!withoutFragment) return true;
  if (withoutFragment.startsWith("/")) {
    record(file, `root-relative reference is not project-Pages safe: ${reference}`);
    return false;
  }
  const decoded = decodeURIComponent(withoutFragment);
  const target = normalize(resolve(dirname(file), decoded));
  const relativeToDocs = relative(documentationRoot, target);
  if (relativeToDocs.startsWith("..") || relativeToDocs.includes(`..${process.platform === "win32" ? "\\" : "/"}`)) {
    record(file, `reference escapes the published docs directory: ${reference}`);
    return false;
  }
  if (existsSync(target) && statSync(target).isFile()) return true;
  if (existsSync(target) && statSync(target).isDirectory() && existsSync(join(target, "index.html"))) return true;
  record(file, `missing local target: ${reference}`);
  return false;
}

if (!existsSync(documentationRoot)) throw new Error("docs directory is missing");
walk(documentationRoot);

for (const file of htmlFiles) {
  const html = readFileSync(file, "utf8");
  if (!/<html\s[^>]*lang=["']en["']/i.test(html)) record(file, "missing html lang=en");
  if (!/<meta\s[^>]*name=["']viewport["']/i.test(html)) record(file, "missing viewport meta tag");
  if (!/<title>[^<]+<\/title>/i.test(html)) record(file, "missing non-empty title");
  if (/file:\/\/\//i.test(html)) record(file, "contains a file URL that cannot work on GitHub Pages");

  const references = [...html.matchAll(/\b(?:href|src)=["']([^"']+)["']/gi)].map((match) => match[1]);
  for (const reference of references) {
    if (/^(?:https?:|mailto:|tel:|data:|javascript:|#)/i.test(reference)) continue;
    localTargetExists(file, reference);
  }
}

for (const required of [
  "index.html",
  ".nojekyll",
  "assets/docs.css",
  "assets/docs.js",
  "project-overview/index.html",
  "getting-started/index.html",
  "architecture/index.html",
  "evidence/index.html",
  "security/index.html",
  "operations/index.html",
  "validation/index.html",
  "research/index.html",
]) {
  if (!existsSync(join(documentationRoot, required))) failures.push(`docs/${required}: required publication file is missing`);
}

if (failures.length) {
  console.error(`Documentation validation failed with ${failures.length} problem(s):`);
  failures.forEach((failure) => console.error(`- ${failure}`));
  process.exitCode = 1;
} else {
  console.log(`Documentation validation passed: ${htmlFiles.length} HTML files and all local references resolved.`);
}
