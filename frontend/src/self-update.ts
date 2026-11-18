// In-app gateway update flow: pure helpers shared by the About settings tab
// and its restart polling. Kept free of React/DOM so node:test can exercise it.

export type UpdateCheckResult = {
  current_version: string
  latest_version: string
  update_available: boolean
  release_notes: string
  published_at: string | null
  release_url: string
  platform_supported: boolean
}

export function formatVersion(version: string) {
  return version ? `v${version}` : '—'
}

export function updateButtonLabel(
  phase: 'idle' | 'checking' | 'applying' | 'restarting',
  check: UpdateCheckResult | null,
  labels: { check: string; checking: string; apply: string; applying: string; restarting: string },
) {
  if (phase === 'checking') return labels.checking
  if (phase === 'applying') return labels.applying
  if (phase === 'restarting') return labels.restarting
  if (check?.update_available) return labels.apply
  return labels.check
}

export function updateActionEnabled(
  phase: 'idle' | 'checking' | 'applying' | 'restarting',
  check: UpdateCheckResult | null,
) {
  if (phase !== 'idle') return false
  if (!check) return true
  return check.update_available ? check.platform_supported : true
}

export function restartFinished(previousVersion: string, observedVersion: string) {
  return Boolean(previousVersion) && Boolean(observedVersion) && observedVersion !== previousVersion
}

export async function waitForRestart(
  previousVersion: string,
  fetchVersion: () => Promise<string>,
  options: { intervalMs?: number; timeoutMs?: number; sleep?: (ms: number) => Promise<void>; now?: () => number } = {},
) {
  const intervalMs = options.intervalMs ?? 1500
  const timeoutMs = options.timeoutMs ?? 120000
  const sleep = options.sleep ?? ((ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms)))
  const now = options.now ?? Date.now
  const deadline = now() + timeoutMs
  for (;;) {
    await sleep(intervalMs)
    let version = ''
    try { version = await fetchVersion() } catch { /* Gateway is restarting; keep polling. */ }
    if (restartFinished(previousVersion, version)) return true
    if (now() >= deadline) return false
  }
}