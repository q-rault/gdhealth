import { defineDocs, defineConfig } from 'fumadocs-mdx/config';
import remarkCodeSnippet from './lib/remark-code-snippet';

export const { docs, meta } = defineDocs({
  dir: 'content/docs',
});

export default defineConfig({
  mdxOptions: {
    remarkPlugins: [remarkCodeSnippet],
  },
});
