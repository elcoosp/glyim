import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export async function resolve(specifier, context, nextResolve) {
  if (specifier.endsWith('.svg')) {
    const result = await nextResolve(specifier, context);
    return { ...result, format: 'module', shortCircuit: true };
  }
  return nextResolve(specifier, context);
}

export async function load(url, context, nextLoad) {
  if (url.endsWith('.svg')) {
    const filePath = fileURLToPath(url);
    const source = readFileSync(filePath, 'utf8');
    const dataUrl = `data:image/svg+xml;base64,${Buffer.from(source).toString('base64')}`;
    return {
      format: 'module',
      source: `export default ${JSON.stringify(dataUrl)};`,
      shortCircuit: true,
    };
  }
  return nextLoad(url, context);
}
