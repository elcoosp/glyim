import { defineConfig } from '@rspress/core';
import path from 'path';

export default defineConfig({
  root: 'docs',
  title: 'Glyim Documentation',
  description: 'Glyim programming language documentation',
  icon: '/favicon.ico',
  logo: {
    light: '/favicon.ico',
    dark: '/favicon.ico',
  },
  globalStyles: path.join(__dirname, 'tailwind.css'),
  themeConfig: {
    socialLinks: [
      { icon: 'github', mode: 'link', content: 'https://github.com/your-repo' }
    ],
    footer: {
      message: 'Built with Glyim and Rspress',
    },
  },
  builderConfig: {
    source: {
      alias: {
        '@components': path.join(__dirname, 'components'),
        '@lib': path.join(__dirname, 'lib'),
        '@': path.join(__dirname),
      },
    },
  },
});
