// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import astroTakumi from 'astro-takumi';
import fs from 'node:fs';
import { makeRender } from './src/og-render.mjs';
import docLinks from 'starlight-links-validator';
import imageZoom from 'starlight-image-zoom';
import headingBadges from 'starlight-heading-badges';

const __logoB64 = fs.readFileSync("./src/assets/logo.png").toString("base64");
const __render = makeRender({
  logoDataUrl: "data:image/png;base64," + __logoB64,
  accentHex: "#743e3e",
  accentHexA12: "#743e3e1a",
  domain: "glyim.vercel.app",
  projectName: "Glyim",
});

// https://astro.build/config
export default defineConfig({
  vite: {
    ssr: { external: ["jsdom", "astro-takumi"] },
  },
  site: 'https://glyim.vercel.app',
        integrations: [
                starlight({
                        routeMiddleware: "./src/routeData.ts",
                        expressiveCode: {
                                themes: ['github-light', 'github-dark'],
                        },
                        title: 'Glyim',
                        description:
                                'Documentation for Glyim, a modular, from-scratch compiler for a Rust-like systems programming language, written in Rust.',

                        logo: { src: './src/assets/logo.png' },
                        favicon: '/favicon.png',
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

                astroTakumi({
                        options: {
                                domBackend: "linkedom",
                                fonts: [fs.readFileSync("./node_modules/@fontsource/roboto/files/roboto-latin-400-normal.woff2")],
                                width: 1200,
                                height: 630,
                                format: "png",
                        },
                        render: __render,
                }),
        ],
});
