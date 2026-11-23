import { workspaceDomains, type WorkspaceDomain } from './workspace-data.ts'

/** Domains named by a live event; `'all'` asks for a full workspace read. */
export type LiveRefreshDomains = readonly WorkspaceDomain[] | 'all'

export type LiveRefreshHandler = (
  domains: LiveRefreshDomains,
  background: boolean,
) => Promise<void>

const knownDomains = new Set<string>(workspaceDomains)

/**
 * Parses one `/api/v1/events` data frame. An empty domain list comes from the
 * gateway when a subscriber lagged behind the broadcast channel, so it means
 * "refresh everything" just like an explicit `"all"`. Malformed or wholly
 * unknown frames yield `null` and are dropped.
 */
export function parseLiveEvent(raw: string): LiveRefreshDomains | null {
  let payload: unknown
  try {
    payload = JSON.parse(raw)
  } catch {
    return null
  }
  if (typeof payload !== 'object' || payload === null) return 'all'
  if (!('domains' in payload)) return 'all'
  const domains = payload.domains
  if (!Array.isArray(domains) || domains.length === 0) return 'all'
  if (domains.includes('all')) return 'all'
  const selected: WorkspaceDomain[] = []
  for (const domain of domains) {
    if (typeof domain === 'string' && knownDomains.has(domain) && !selected.includes(domain as WorkspaceDomain)) {
      selected.push(domain as WorkspaceDomain)
    }
  }
  return selected.length > 0 ? selected : null
}

export const liveRefreshDelayMs = 400

type Waiter = { resolve: () => void; reject: (cause: unknown) => void }

/**
 * Debounces workspace refreshes from live events and from local mutations so a
 * burst of pushes costs one workspace read. Waiters resolve after the batch
 * handler finishes and reject with its error, which keeps `run(...)` awaiting
 * the same refresh semantics as an immediate call.
 */
export class LiveRefreshCoalescer {
  private domains = new Set<WorkspaceDomain>()
  private everything = false
  private background = true
  private timer: ReturnType<typeof setTimeout> | undefined
  private waiters: Waiter[] = []

  private readonly handler: LiveRefreshHandler
  private readonly delayMs: number

  constructor(handler: LiveRefreshHandler, delayMs = liveRefreshDelayMs) {
    this.handler = handler
    this.delayMs = delayMs
  }

  schedule(domains: LiveRefreshDomains, background = true): Promise<void> {
    if (domains === 'all') this.everything = true
    else for (const domain of domains) this.domains.add(domain)
    // A batch containing any foreground caller must report failures like an
    // immediate refresh always did.
    this.background = this.background && background
    // `Promise.withResolvers` is ES2024; this project targets ES2020 lib.
    const promise = new Promise<void>((resolve, reject) => this.waiters.push({ resolve, reject }))
    if (this.timer === undefined) {
      this.timer = setTimeout(() => {
        this.timer = undefined
        void this.flush()
      }, this.delayMs)
    }
    return promise
  }

  /** Drops a pending batch, e.g. when the console leaves the session. */
  cancel() {
    clearTimeout(this.timer)
    this.timer = undefined
    this.everything = false
    this.domains.clear()
    this.background = true
    const waiters = this.waiters
    this.waiters = []
    for (const waiter of waiters) waiter.resolve()
  }

  private async flush() {
    const domains: LiveRefreshDomains = this.everything ? 'all' : [...this.domains]
    const background = this.background
    const waiters = this.waiters
    this.everything = false
    this.domains.clear()
    this.background = true
    this.waiters = []
    try {
      await this.handler(domains, background)
      for (const waiter of waiters) waiter.resolve()
    } catch (cause) {
      for (const waiter of waiters) waiter.reject(cause)
    }
  }
}