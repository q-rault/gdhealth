import { source } from '@/lib/source';
import { createFromSource } from 'fumadocs-core/search/server';

// Built once into a static JSON index so search works on GitHub Pages,
// where no server answers queries; the browser searches the index itself.
export const revalidate = false;

export const { staticGET: GET } = createFromSource(source);
