//! Client address resolution for deployments behind a reverse proxy.
//!
//! Docker port mapping (and any reverse proxy) makes the TCP peer the proxy's
//! address, so the recorded client IP and the policy whitelist would otherwise
//! see the bridge gateway instead of the real caller. When the gateway is
//! explicitly configured as a trusted-proxy deployment, the forwarded headers
//! are authoritative; otherwise they are ignored so a direct client can never
//! spoof its address.

use axum::http::HeaderMap;
use std::net::IpAddr;

/// Resolves the caller address from the forwarding headers.
///
/// `X-Forwarded-For` is a comma-separated hop list whose leftmost entry is the
/// original client, so the first parsable entry wins. `X-Real-IP` covers proxies
/// that send only that header. Anything missing or unparsable falls back to the
/// transport peer. Without `trusted_proxy` the peer is always returned.
pub(crate) fn client_ip(headers: &HeaderMap, peer: IpAddr, trusted_proxy: bool) -> IpAddr {
    if !trusted_proxy {
        return peer;
    }
    let forwarded = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').find_map(parse_address));
    if let Some(address) = forwarded {
        return address;
    }
    if let Some(address) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_address)
    {
        return address;
    }
    peer
}

/// Strict IP parsing that also accepts the `ip:port` and `[v6]:port` forms some
/// proxies emit. Bare IPv4/IPv6 values parse directly; anything else is invalid.
fn parse_address(value: &str) -> Option<IpAddr> {
    let value = value.trim();
    if let Ok(address) = value.parse::<IpAddr>() {
        return Some(address);
    }
    if let Some(rest) = value.strip_prefix('[') {
        let (host, _) = rest.split_once(']')?;
        return host.parse::<IpAddr>().ok();
    }
    let (host, port) = value.rsplit_once(':')?;
    if port.is_empty() || !port.chars().all(|digit| digit.is_ascii_digit()) {
        return None;
    }
    host.parse::<IpAddr>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(values: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in values {
            headers.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        headers
    }

    fn peer() -> IpAddr {
        "172.17.0.1".parse().unwrap()
    }

    #[test]
    fn ignores_forwarding_headers_unless_trusted() {
        let forwarded = headers(&[
            ("x-forwarded-for", "203.0.113.7"),
            ("x-real-ip", "198.51.100.9"),
        ]);
        assert_eq!(client_ip(&forwarded, peer(), false), peer());
    }

    #[test]
    fn prefers_the_leftmost_forwarded_address() {
        let forwarded = headers(&[
            ("x-forwarded-for", "203.0.113.7, 10.0.0.1, 172.17.0.1"),
            ("x-real-ip", "198.51.100.9"),
        ]);
        assert_eq!(
            client_ip(&forwarded, peer(), true),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn falls_back_to_real_ip_then_peer() {
        let real_ip = headers(&[("x-real-ip", "198.51.100.9")]);
        assert_eq!(
            client_ip(&real_ip, peer(), true),
            "198.51.100.9".parse::<IpAddr>().unwrap()
        );
        assert_eq!(client_ip(&HeaderMap::new(), peer(), true), peer());
        let malformed = headers(&[("x-forwarded-for", "not-an-ip"), ("x-real-ip", "nope")]);
        assert_eq!(client_ip(&malformed, peer(), true), peer());
    }

    #[test]
    fn accepts_port_variants() {
        assert_eq!(
            parse_address("198.51.100.9:443"),
            Some("198.51.100.9".parse().unwrap())
        );
        assert_eq!(
            parse_address("[2001:db8::7]:8443"),
            Some("2001:db8::7".parse().unwrap())
        );
        assert_eq!(
            parse_address("2001:db8::7"),
            Some("2001:db8::7".parse().unwrap())
        );
        assert_eq!(parse_address("203.0.113.7:not-a-port"), None);
    }
}
