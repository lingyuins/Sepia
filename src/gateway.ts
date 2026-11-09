export type GatewayAddress = {
  deployment: 'local' | 'cloud'
  protocol: 'http' | 'https'
  host: string
  port: string
}

export function gatewayUrl(address: GatewayAddress, path = '/v1') {
  const protocol = address.deployment === 'local' ? 'http' : address.protocol
  const url = new URL(`${protocol}://${address.host.trim()}`)
  url.port = address.port
  url.pathname = path
  return url.toString()
}
