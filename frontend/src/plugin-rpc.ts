export function pluginRpcPath(pluginId: string, method: string): string {
  const normalized = method.replace(/^\/+/, '')
  const segments = normalized.split('/')
  if (
    !normalized ||
    /[\s\\%?#]/.test(normalized) ||
    Array.from(normalized).some((character) => character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127) ||
    segments.some((segment) => !segment || /^\.+$/.test(segment))
  ) {
    throw new Error('Invalid plugin RPC method')
  }
  return `/api/v1/plugins/${encodeURIComponent(pluginId)}/rpc/${segments.map(encodeURIComponent).join('/')}`
}
