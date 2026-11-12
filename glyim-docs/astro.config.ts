import { defineDocsConfig } from '@elcoosp/docs-toolkit/config';
import starlightChangelogs from 'starlight-changelogs';
import docLinks from 'starlight-links-validator';
import imageZoom from 'starlight-image-zoom';
import headingBadges from 'starlight-heading-badges';

export default await defineDocsConfig({
  llmsTxt: {},
  site: 'https://glyim.vercel.app',
  domain: 'glyim.vercel.app',
  title: 'Glyim',
  description:
    'Documentation for Glyim, a modular, from-scratch compiler for a Rust-like systems programming language, written in Rust.',
  social: [
    { icon: 'github', label: 'GitHub', href: 'https://github.com/elcoosp/glyim' },
  ],
  editLink: {
    baseUrl: 'https://github.com/elcoosp/glyim/edit/main/glyim-docs/src/content/docs/',
  },
  locales: { root: { label: 'English', lang: 'en' } },
    plugins: [ starlightChangelogs(), docLinks(), imageZoom(), headingBadges()],
  sidebar: [
    { label: 'Overview', link: '/' },
    { label: 'Tutorials', items: [{ autogenerate: { directory: 'tutorials' } }] },
    { label: 'How-to Guides', items: [{ autogenerate: { directory: 'guides' } }] },
    { label: 'Reference', items: [{ autogenerate: { directory: 'reference' } }] },
    { label: 'Explanation', items: [{ autogenerate: { directory: 'explanation' } }] },
  ],
});
