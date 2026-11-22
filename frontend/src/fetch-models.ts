// Pure helpers for the provider "fetch models" dialog. Detection runs against
// every key of the provider's first address (unified channels share one model
// catalog across addresses); results are merged into a union and kept per key.

export type KeyDetection = { keyId: number; models: string[] | null; error: string | null }

/** Union of every successful key's catalog, de-duplicated, first-seen order. */
export function mergeDetectedModels(results: KeyDetection[]): string[] {
  const seen: string[] = []
  for (const result of results) {
    for (const model of result.models || []) {
      if (!seen.includes(model)) seen.push(model)
    }
  }
  return seen
}

/** Case-insensitive substring filter shared by the union and per-key lists. */
export function filterModels(models: string[], query: string): string[] {
  const needle = query.trim().toLowerCase()
  if (!needle) return models
  return models.filter((model) => model.toLowerCase().includes(needle))
}

/** Merge a selection into the configured list without duplicates, keeping order. */
export function addSelectedModels(current: string[], selected: string[]): string[] {
  const merged = [...current]
  for (const model of selected) {
    if (!merged.includes(model)) merged.push(model)
  }
  return merged
}