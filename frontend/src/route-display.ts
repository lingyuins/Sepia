import type { RequestRecord, RouteCandidate, RouteRule } from './workspace'

export function routeDisplayCandidates(route: RouteRule): RouteCandidate[] {
  return (route.candidates || []).filter((candidate) => candidate.enabled)
}

export function latestSuccessfulRouteRequest(route: RouteRule, requests: RequestRecord[]) {
  return requests.find((request) => request.route === route.slug && request.status === '200 OK')
}

export function selectOverviewRoute(routes: RouteRule[], requests: RequestRecord[]) {
  const enabled = routes.filter((route) => route.active)
  const latest = requests.reduce<RequestRecord | undefined>((current, request) => enabled.some((route) => route.slug === request.route) && (!current || Date.parse(request.createdAt) > Date.parse(current.createdAt)) ? request : current, undefined)
  return enabled.find((route) => route.slug === latest?.route) || enabled[0]
}

/** Slug/name for duplicating a route: first copy gains `-copy`/(copy suffix),
 * later copies gain `-copy-2`, `-copy-3`… against every existing slug, with the
 * slug capped at 64 characters (the base is shortened when the suffix needs the
 * room, mirroring backend/src/ai.rs disambiguate_slug). The display name gains
 * the same number so repeated copies stay distinguishable. */
export function duplicateRouteIdentity(
  route: RouteRule,
  existingSlugs: string[],
  copySuffix: string,
): { slug: string; name: string } {
  const used = new Set(existingSlugs)
  const baseSlug = route.slug || route.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'route'
  const baseName = `${route.name} ${copySuffix}`
  const firstSuffix = '-copy'
  const firstRoom = Math.max(1, 64 - firstSuffix.length)
  const firstBase = baseSlug.slice(0, firstRoom).replace(/-+$/, '') || 'route'
  const first = `${firstBase}${firstSuffix}`
  if (!used.has(first)) return { slug: first, name: baseName }
  let suffix = 2
  for (;;) {
    const suffixText = `-copy-${suffix}`
    const room = Math.max(1, 64 - suffixText.length)
    const base = baseSlug.slice(0, room).replace(/-+$/, '') || 'route'
    const candidate = `${base}${suffixText}`
    if (!used.has(candidate)) return { slug: candidate, name: `${baseName} ${suffix}` }
    suffix += 1
  }
}
