import { defineConfig } from 'rspress/config';

export default defineConfig({
  root: '.',
  title: 'Glyim Documentation',
  description: 'Glyim programming language documentation',
  icon: '/favicon.ico',
  themeConfig: {
    socialLinks: [
      { icon: 'github', mode: 'link', content: 'https://github.com/your-repo' }
    ],
    footer: {
      message: 'Made with Rspress and Glyim',
    },
  },
});
