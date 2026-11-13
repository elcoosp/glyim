import { defineDocsConfig } from '@elcoosp/docs-toolkit/config';
import imageZoom from 'starlight-image-zoom';
import headingBadges from 'starlight-heading-badges';

export default await defineDocsConfig({
  extraStarlight: {
    components: {
      MarkdownContent: './src/components/MarkdownContent.astro',
    },
  },
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
  locales: { root: { label: 'English', lang: 'en' }, fr: { label: 'Français', lang: 'fr' } },
    plugins: [imageZoom(),  headingBadges()],
  sidebar: [
    { label: 'Overview', translations: { fr: 'Aperçu' }, link: '/' },
    { label: 'Tutorials', translations: { fr: 'Tutoriels' }, items: [{ autogenerate: { directory: 'tutorials' } }] },
    { label: 'How-to Guides', translations: { fr: 'Guides pratiques' }, items: [{ autogenerate: { directory: 'guides' } }] },
    { label: 'Reference', translations: { fr: 'Référence' }, items: [{ autogenerate: { directory: 'reference' } }] },
    { label: 'Explanation', translations: { fr: 'Explication' }, items: [{ autogenerate: { directory: 'explanation' } }] },
  ],
  vite: { ssr: { noExternal: ['starlight-changelogs'] } },
});
