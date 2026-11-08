// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import docLinks from 'starlight-links-validator';
import imageZoom from 'starlight-image-zoom';
import headingBadges from 'starlight-heading-badges';

// https://astro.build/config
export default defineConfig({
        integrations: [
                starlight({
                        title: 'Glyim',
                        description:
                                'Documentation for Glyim, a modular, from-scratch compiler for a Rust-like systems programming language, written in Rust.',
                        logo: {
                                src: './src/assets/logo.png',
                                alt: 'Glyim',
                                replacesTitle: false,
                        },
                        favicon: '/favicon.png',
                        head: [
                                { tag: 'link', attrs: { rel: 'icon', type: 'image/png', href: '/favicon.png' } },
                                { tag: 'link', attrs: { rel: 'apple-touch-icon', href: '/apple-touch-icon.png' } },
                        ],
                        customCss: ['./src/styles/custom.css'],
                        social: [
                                { icon: 'github', label: 'GitHub', href: 'https://github.com/elcoosp/glyim' },
                        ],
                        editLink: {
                                baseUrl: 'https://github.com/elcoosp/glyim/edit/main/docs-site/src/content/docs/',
                        },
                        locales: {
                                en: { label: 'English' },
                        },
                        defaultLocale: 'en',
                        plugins: [docLinks(), imageZoom(), headingBadges()],
                        sidebar: [
                                { label: 'Overview', link: '/' },
                                {
                                        label: 'Tutorials',
                                        items: [{ autogenerate: { directory: 'tutorials' } }],
                                },
                                {
                                        label: 'How-to Guides',
                                        items: [{ autogenerate: { directory: 'guides' } }],
                                },
                                {
                                        label: 'Reference',
                                        items: [{ autogenerate: { directory: 'reference' } }],
                                },
                                {
                                        label: 'Explanation',
                                        items: [{ autogenerate: { directory: 'explanation' } }],
                                },
                        ],
                }),
        ],
});
