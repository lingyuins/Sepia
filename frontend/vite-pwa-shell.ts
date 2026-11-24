import { readFileSync, writeFileSync } from 'node:fs'
import type { Plugin } from 'vite'

// PWA: precache the built entry assets into sw.js so a first visit can boot
// offline after the service worker activates — without waiting for a second
// controlled load to fill the cache. Entry chunks plus their static imports
// (JS/CSS/fonts) are listed, and so are the chunks behind the entry's dynamic
// imports (the analytics charts) with their own static closure: those modules
// are needed offline too, and a failed chunk fetch would blank the charts.
// Dynamic imports nested deeper stay cache-on-first-use.
export function pwaShell(): Plugin {
  return {
    name: 'sepia-pwa-shell',
    apply: 'build',
    writeBundle(options, bundle) {
      const files = new Set<string>()
      const visit = (fileName: string) => {
        const chunk = bundle[fileName]
        if (chunk?.type !== 'chunk' || files.has(fileName)) return
        files.add(fileName)
        for (const css of chunk.viteMetadata?.importedCss ?? []) files.add(css)
        for (const asset of chunk.viteMetadata?.importedAssets ?? []) files.add(asset)
        for (const imported of chunk.imports) if (bundle[imported]?.type === 'chunk') visit(imported)
      }
      // A dynamic import may point at a chunk or a plain asset (CSS); the
      // latter is cached as-is since it has no static closure of its own.
      const visitDynamic = (fileName: string) => {
        if (bundle[fileName]?.type === 'chunk') visit(fileName)
        else files.add(fileName)
      }
      for (const item of Object.values(bundle)) {
        if (item.type !== 'chunk' || !item.isEntry) continue
        visit(item.fileName)
        for (const lazy of item.dynamicImports) visitDynamic(lazy)
      }
      const urls = [...files].map((file) => `/${file}`).sort()
      const swPath = `${options.dir ?? 'dist'}/sw.js`
      const source = readFileSync(swPath, 'utf8')
      const patched = source.replace('const PRECACHE_ASSETS = []', `const PRECACHE_ASSETS = ${JSON.stringify(urls)}`)
      if (patched === source) throw new Error('sw.js is missing the PRECACHE_ASSETS placeholder')
      writeFileSync(swPath, patched)
    },
  }
}