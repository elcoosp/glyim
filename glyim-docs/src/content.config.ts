import { glob } from 'astro/loaders';
import { defineCollection, z } from 'astro:content';
import { docsLoader, i18nLoader } from '@astrojs/starlight/loaders';
import { changelogsLoader } from 'starlight-changelogs/loader';
import { blogSchema } from 'starlight-blog/schema';
import { docsSchema, i18nSchema } from '@astrojs/starlight/schema';


const docsMetadataCollection = defineCollection({
  loader: glob({ base: './src/content/docs', pattern: '**/_meta.{yml,yaml}' }),
  schema: z.object({
    label: z.string().optional(),
    order: z.array(z.string()).optional(),
    sort: z.enum(['label','filename']).optional(),
    collapsed: z.boolean().optional(),
    hidden: z.boolean().optional(),
    depth: z.number().optional(),
    cascade: z.record(z.string(), z.unknown()).optional(),
  }).passthrough(),
});

export const collections = {
  docsMetadata: docsMetadataCollection,
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
