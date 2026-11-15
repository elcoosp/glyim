import { glob } from 'astro/loaders';
import { defineCollection, z } from 'astro:content';
import { docsLoader, i18nLoader } from '@astrojs/starlight/loaders';
import { changelogsLoader } from 'starlight-changelogs/loader';
import { blogSchema } from 'starlight-blog/schema';
import { docsSchema, i18nSchema } from '@astrojs/starlight/schema';
import { autoSidebarLoader } from 'starlight-auto-sidebar/loader';
import { autoSidebarSchema } from 'starlight-auto-sidebar/schema';


export const collections = {
  autoSidebar: defineCollection({ loader: autoSidebarLoader(), schema: autoSidebarSchema() }),
  changelogs: defineCollection({
    loader: changelogsLoader([
      {
        provider: 'keep-a-changelog',
        base: 'changelog',
        changelog: '../CHANGELOG.md',
        title: 'Changelog',
      },
    ]),
  }),
  docs: defineCollection({
    loader: docsLoader(),
    schema: docsSchema({
        extend: (context) => {
          const blog = blogSchema(context) as any;
          const patchedDate = z.preprocess(
            (v) => (v instanceof Date ? v : typeof v === "string" ? new Date(v) : v),
            z.date(),
          ).optional();
          if (blog && typeof blog === "object" && typeof blog.extend === "function") {
            return blog.extend({ date: patchedDate });
          }
          return blog;
        },
      }),
  }),
  i18n: defineCollection({ loader: i18nLoader(), schema: i18nSchema() }),
};
