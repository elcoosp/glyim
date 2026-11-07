// Generates dist/llms.txt and dist/llms-full.txt after `astro build`.
// See docs/decisions/0005-llms-txt.md.
import { readFileSync, readdirSync, statSync, writeFileSync, existsSync } from 'node:fs';
import { join, relative, dirname, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const docsDir = join(root, 'src', 'content', 'docs');
const distDir = join(root, 'dist');

const SECTIONS = [
  { key: 'tutorials', title: 'Tutorials', type: 'tutorial' },
  { key: 'guides', title: 'How-to Guides', type: 'how-to' },
  { key: 'reference', title: 'Reference', type: 'reference' },
  { key: 'explanation', title: 'Explanation', type: 'explanation' },
];

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (/\.(md|mdx)$/.test(entry)) out.push(p);
  }
  return out;
}

function parseFrontmatter(text) {
  if (!text.startsWith('---')) return {};
  const end = text.indexOf('\n---', 3);
  if (end === -1) return {};
  const block = text.slice(3, end);
  const fm = {};
  for (const line of block.split('\n')) {
    const m = line.match(/^(\w[\w-]*):\s*"?(.*?)"?\s*$/);
    if (m) fm[m[1]] = m[2].trim();
  }
  return fm;
}

function routeFor(file) {
  const rel = relative(docsDir, file).split(sep).join('/');
  const base = rel.replace(/\.(md|mdx)$/, '');
  if (base === 'index') return '/';
  return `/${base}/`;
}

function stripForLlms(text) {
  // Remove frontmatter.
  let body = text.replace(/^---\n[\s\S]*?\n---\n/, '');
  // Remove MDX import statements and JSX component lines.
  body = body
    .split('\n')
    .filter((l) => !/^import\s+.*from\s+['"]@astrojs\/starlight/.test(l))
    .join('\n');
  return body.trim();
}

const files = walk(docsDir);
const pages = files.map((f) => {
  const raw = readFileSync(f, 'utf8');
  const fm = parseFrontmatter(raw);
  return {
    file: f,
    title: fm.title || relative(docsDir, f),
    description: fm.description || '',
    route: routeFor(f),
    body: stripForLlms(raw),
  };
});

// Landing page first, then sections in Diátaxis order.
const landing = pages.filter((p) => p.route === '/');
const sectionOf = (p) => SECTIONS.find((s) => p.route.startsWith(`/${s.key}/`));
const ordered = [
  ...landing,
  ...SECTIONS.flatMap((s) =>
    pages
      .filter((p) => sectionOf(p)?.key === s.key)
      .sort((a, b) => a.route.localeCompare(b.route))
  ),
];

const header = [
  `# Glyim`,
  '',
  'Glyim is a modular, from-scratch compiler for a Rust-like systems programming language, written in Rust. It implements a complete pipeline — lexing, parsing, name resolution, HIR, MIR, type inference and trait solving, NLL borrow checking, optimizations, and LLVM plus bytecode backends — with the glyip build tool, a language server, and a five-mode test harness. The docs follow the Diátaxis framework.',
  '',
].join('\n');

let llms = header;
for (const s of SECTIONS) {
  const items = ordered.filter((p) => sectionOf(p)?.key === s.key);
  if (!items.length) continue;
  llms += `\n## ${s.title}\n\n`;
  for (const p of items) {
    llms += `- [${p.title}](${p.route}): ${p.description}\n`;
  }
}
// Pages not in any known section (defensive).
const rest = ordered.filter((p) => p.route !== '/' && !sectionOf(p));
if (rest.length) {
  llms += `\n## Other\n\n`;
  for (const p of rest) llms += `- [${p.title}](${p.route}): ${p.description}\n`;
}

let full = `${header}\n---\n\n`;
for (const p of ordered) {
  full += `<!-- ${p.route} -->\n\n${p.body}\n\n---\n\n`;
}

if (!existsSync(distDir)) {
  console.error('dist/ missing — run `astro build` first.');
  process.exit(1);
}
writeFileSync(join(distDir, 'llms.txt'), llms);
writeFileSync(join(distDir, 'llms-full.txt'), full);
console.log(`llms.txt (${llms.length} B) and llms-full.txt (${full.length} B) written to dist/`);
