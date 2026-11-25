import { defineDocsConfig } from '@elcoosp/docs-toolkit/config';

function pickMascot(pathname = '', section = 'Overview') {
  const p = ('/' + pathname.toLowerCase().replace(/^\/+/, '')).replace(/^\/fr(?=\/|$)/, '') || '/';

  if (p.includes('404') || section === '404') {
    return { src: './public/mascot-404.svg', placement: 'right-hero', size: 310 };
  }
  if (p.includes('borrow')) {
    return { src: './public/mascot-borrowck.svg', placement: 'right-hero', size: 310 };
  }
  if (p.includes('gsx')) {
    return { src: './public/mascot-gsx.svg', placement: 'right-hero', size: 310 };
  }
  if (p.includes('lsp')) {
    return { src: './public/mascot-lsp.svg', placement: 'right-hero', size: 310 };
  }
  if (p.includes('ffi')) {
    return { src: './public/mascot-ffi.svg', placement: 'right-hero', size: 310 };
  }
  if (p.includes('concurren') || p.includes('async')) {
    return { src: './public/mascot-concurrency.svg', placement: 'right-hero', size: 310 };
  }
  if (p.includes('glyip')) {
    return { src: './public/mascot-glyip.svg', placement: 'right-hero', size: 310 };
  }
  if (p.includes('lint') || p.includes('warn')) {
    return { src: './public/mascot-warn.svg', placement: 'right-hero', size: 310 };
  }
  if (p.includes('danger') || p.includes('unsafe')) {
    return { src: './public/mascot-danger.svg', placement: 'right-hero', size: 310 };
  }
  if (
    p.includes('pipeline') ||
    p.includes('architecture') ||
    p.includes('crates') ||
    p.includes('hir') ||
    p.includes('mir') ||
    p.includes('llvm')
  ) {
    return { src: './public/mascot-pipeline.svg', placement: 'right-hero', size: 310 };
  }
  if (p.includes('test')) {
    return { src: './public/mascot-test.svg', placement: 'right-hero', size: 310 };
  }
  if (p.includes('speed') || p.includes('perf') || p.includes('optim') || p.startsWith('/changelog')) {
    return { src: './public/mascot-speed.svg', placement: 'right-hero', size: 310 };
  }
  if (p.includes('communit') || p.includes('contribut') || p.startsWith('/blog')) {
    return { src: './public/mascot-community.svg', placement: 'right-hero', size: 310 };
  }

  if (p.startsWith('/tutorials/') || p.startsWith('/getting-started/') || section === 'Tutorial') {
    return { src: './public/mascot-tip.svg', placement: 'right-hero', size: 310 };
  }
  if (p.startsWith('/guides/') || section === 'Guide') {
    return { src: './public/mascot-guide.svg', placement: 'right-hero', size: 310 };
  }
  if (p.startsWith('/reference/') || section === 'Reference') {
    return { src: './public/mascot-note.svg', placement: 'right-hero', size: 310 };
  }
  if (p.startsWith('/explanation/') || p.startsWith('/concepts/') || section === 'Explanation') {
    return { src: './public/mascot-pipeline.svg', placement: 'right-hero', size: 310 };
  }

  return { src: './public/mascot.svg', placement: 'watermark', size: 440 };
}

export default await defineDocsConfig({
  topics: true,
  site: 'https://vercel.app',
  domain: 'glyim.vercel.app',
  github: 'elcoosp/glyim',
  title: 'Glyim',
  logoPath: './src/assets/logo.png',
  favicon: '/logo.svg',
  og: {
    fontFamily: 'Space Grotesk',
    monoFontFamily: 'JetBrains Mono',
    fonts: [
      {
        src: './node_modules/@fontsource/space-grotesk/files/space-grotesk-latin-400-normal.woff',
        family: 'Space Grotesk',
        role: 'sans',
      },
      {
        src: './node_modules/@fontsource/space-grotesk/files/space-grotesk-latin-700-normal.woff',
        family: 'Space Grotesk',
        role: 'sans',
      },
      {
        src: './node_modules/@fontsource/jetbrains-mono/files/jetbrains-mono-latin-400-normal.woff',
        family: 'JetBrains Mono',
        role: 'mono',
      },
      {
        src: './node_modules/@fontsource/jetbrains-mono/files/jetbrains-mono-latin-500-normal.woff',
        family: 'JetBrains Mono',
        role: 'mono',
      },
    ],
    illustration: ({ pathname, section }) => pickMascot(pathname, section),
  },
  extraStarlight: {
    logo: {
      src: './src/assets/logo.svg',
      alt: 'Glyim',
    },
    head: [
      {
        tag: 'link',
        attrs: { rel: 'icon', type: 'image/svg+xml', href: '/logo.svg' },
      },
    ],
  },
  description:
    'Documentation for Glyim, a modular, from-scratch compiler for a Rust-like systems programming language, written in Rust.',
});
