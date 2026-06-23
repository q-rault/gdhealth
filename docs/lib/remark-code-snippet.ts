import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

// Paths are resolved from the repo root (process.cwd()), not from the MDX file.
// Use: file="tools/gdhealth/tests/docs/gdhealth.example.toml" region="full-config"
const ROOT = process.cwd()

function parseCodeMeta(meta: string): Record<string, string> {
  const attrs: Record<string, string> = {}
  const re = /(\w+)="([^"]*)"/g
  let m: RegExpExecArray | null
  while ((m = re.exec(meta)) !== null) {
    attrs[m[1]] = m[2]
  }
  return attrs
}

function extractRegion(source: string, region: string, filePath: string): string {
  const lines = source.split('\n')
  const start = lines.findIndex(l => l.trim() === `# region: ${region}`)
  if (start === -1) throw new Error(`Region "${region}" not found in ${filePath}`)
  const end = lines.findIndex((l, i) => i > start && l.trim() === '# endregion')
  if (end === -1) throw new Error(`No # endregion after region "${region}" in ${filePath}`)
  return lines.slice(start + 1, end).join('\n').trimEnd()
}

function walk(node: any, fn: (n: any) => void): void {
  fn(node)
  if (Array.isArray(node.children)) {
    for (const child of node.children) walk(child, fn)
  }
}

export default function remarkCodeSnippet() {
  return (tree: any, vfile: any) => {
    walk(tree, (node: any) => {
      if (node.type !== 'code' || !node.meta) return
      const attrs = parseCodeMeta(node.meta as string)
      if (!attrs.file) return
      const absPath = resolve(ROOT, '..', attrs.file)
      const source = readFileSync(absPath, 'utf8')
      node.value = attrs.region
        ? extractRegion(source, attrs.region, absPath)
        : source.trimEnd()
      node.meta = (node.meta as string)
        .replace(/\s*(file|region)="[^"]*"/g, '')
        .trim() || null
    })
  }
}
