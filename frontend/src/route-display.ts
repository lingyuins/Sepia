import type { RequestRecord, RouteCandidate, RouteRule } from './workspace'

export function routeDisplayCandidates(route: RouteRule): RouteCandidate[] {
  const candidates = route.candidates?.length ? route.candidates : [
    { id: `${route.id}-primary`, provider: route.primary, model: route.primaryModel, enabled: true, weight: 1 },
    ...(route.fallback ? [{ id: `${route.id}-fallback`, provider: route.fallback, model: route.fallbackModel, enabled: true, weight: 1 }] : []),
  ]
  return candidates.filter((candidate) => candidate.enabled)
}

export function latestSuccessfulRouteRequest(route: RouteRule, requests: RequestRecord[]) {
  return requests.find((request) => request.route === route.slug && request.status === '200 OK')
}

export function selectOverviewRoute(routes: RouteRule[], requests: RequestRecord[]) {
  const enabled = routes.filter((route) => route.active)
  const latest = requests.reduce<RequestRecord | undefined>((current, request) => enabled.some((route) => route.slug === request.route) && (!current || Date.parse(request.createdAt) > Date.parse(current.createdAt)) ? request : current, undefined)
  return enabled.find((route) => route.slug === latest?.route) || enabled[0]
}
