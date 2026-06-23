import { docs, meta } from '@/.source';
import { createMDXSource, resolveFiles } from 'fumadocs-mdx';
import { loader } from 'fumadocs-core/source';

// fumadocs-mdx v11 declares createMDXSource.files as VirtualFile[] but returns a function;
// fumadocs-core v15 requires an array. We keep the typed source and patch files at runtime.
const src = createMDXSource(docs, meta);
Object.assign(src, { files: resolveFiles({ docs, meta }) });

export const source = loader({
  baseUrl: '/docs',
  source: src,
});
