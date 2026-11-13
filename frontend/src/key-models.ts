import type { Provider, RouteRule } from './workspace'

export function scopedModels(providers: Pick<Provider, 'modelsList'>[], routes: Pick<RouteRule, 'id' | 'active' | 'slug' | 'name'>[]) {
  const routeModels = routes.filter((route) => route.active).map((route) => ({ value: route.slug || String(route.id), label: route.name || route.slug || String(route.id), detail: route.slug || String(route.id), route: true }))
  const providerModels = [...new Set(providers.flatMap((provider) => provider.modelsList))].map((model) => ({ value: model, label: model, detail: '', route: false }))
  return [...routeModels, ...providerModels]
}
