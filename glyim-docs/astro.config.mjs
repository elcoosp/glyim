import { defineDocsConfig } from '@elcoosp/docs-toolkit/config';

export default await defineDocsConfig({
  topics: true,
  site: 'https://vercel.app',
  domain: 'glyim.vercel.app',
  github: 'elcoosp/glyim',
  title: 'Glyim',
  logoPath: './src/assets/logo.png',
  favicon: '/logo.svg',
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
