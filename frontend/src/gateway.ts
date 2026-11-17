export type GatewayAddress = {
  deployment: 'local' | 'cloud'
  protocol: 'http' | 'https'
  host: string
  port: string
}

export function gatewayUrl(address: GatewayAddress, path = '/v1') {
  const protocol = address.deployment === 'local' ? 'http' : address.protocol
  const url = new URL(`${protocol}://${address.host.trim()}`)
  // Cloud HTTPS sits behind the user's reverse proxy on the standard port;
  // the stored port is display-only there. Cloud HTTP needs an explicit port.
  if (!(address.deployment === 'cloud' && protocol === 'https')) url.port = address.port
  url.pathname = path
  return url.toString()
}
