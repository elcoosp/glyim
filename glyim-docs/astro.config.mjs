import { defineDocsConfig } from '@elcoosp/docs-toolkit/config';

export default await defineDocsConfig({
  topics: true,
  site: 'https://vercel.app',
  domain: 'glyim.vercel.app',
  github: 'elcoosp/glyim',
  title: 'Glyim',
  description:
    'Documentation for Glyim, a modular, from-scratch compiler for a Rust-like systems programming language, written in Rust.',
});
