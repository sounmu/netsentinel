//! SSRF protection: validate URLs and hostnames against private/reserved IP ranges.
//!
//! Used by webhook configuration, HTTP monitors, and ping monitors to prevent
//! server-side requests to internal services (RFC 1918, link-local, loopback, etc.).

use std::net::{IpAddr, SocketAddr};

/// Returns `true` if the IP address is in a private, reserved, or link-local range.
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()       // 127.0.0.0/8
            || v4.is_private()     // 10/8, 172.16/12, 192.168/16
            || v4.is_link_local()  // 169.254/16 (AWS metadata etc.)
            || v4.is_unspecified() // 0.0.0.0
            || v4.is_broadcast()   // 255.255.255.255
            // 100.64.0.0/10 — Carrier-grade NAT (RFC 6598)
            || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64)
        }
        IpAddr::V6(v6) => {
            // IPv4-mapped IPv6 (e.g. ::ffff:127.0.0.1) — unwrap and check the inner v4
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_private_ip(IpAddr::V4(mapped));
            }
            v6.is_loopback()       // ::1
            || v6.is_unspecified() // ::
            // fe80::/10 — link-local
            || (v6.segments()[0] & 0xffc0) == 0xfe80
            // fc00::/7 — unique local address (ULA)
            || (v6.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

/// Validate a URL: parse, check scheme, resolve DNS, block private/reserved IPs.
///
/// `allowed_schemes` restricts which URL schemes are accepted (e.g., `&["https"]`).
/// Returns `Ok(())` if the URL is valid and points to a public IP.
pub async fn validate_url(url_str: &str, allowed_schemes: &[&str]) -> Result<(), String> {
    let parsed = url::Url::parse(url_str).map_err(|e| format!("Invalid URL: {e}"))?;

    if !allowed_schemes.contains(&parsed.scheme()) {
        return Err(format!(
            "URL scheme '{}' not allowed (expected: {})",
            parsed.scheme(),
            allowed_schemes.join(", ")
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL must contain a host".to_string())?;

    // Determine port: explicit port in URL, or default for scheme
    let port = parsed.port_or_known_default().unwrap_or(443);

    check_host_ips(host, port).await.map(|_| ())
}

/// Validate a host:port target (for ping monitors): resolve DNS, block private IPs.
///
/// Accepts formats: `hostname`, `hostname:port`, `ip:port`.
pub async fn validate_host(host_str: &str) -> Result<(), String> {
    resolve_public_host(host_str).await.map(|_| ())
}

/// Like [`validate_host`], but returns the addresses that passed the check.
///
/// Callers that open the connection themselves must connect to these
/// addresses rather than resolving the name again: a second lookup can
/// return a different (internal) answer — DNS rebinding.
pub async fn resolve_public_host(host_str: &str) -> Result<Vec<SocketAddr>, String> {
    let (host, port) = if let Some((h, p)) = host_str.rsplit_once(':') {
        let port: u16 = p.parse().map_err(|_| format!("Invalid port: {p}"))?;
        (h, port)
    } else {
        (host_str, 80)
    };

    check_host_ips(host, port).await
}

/// DNS resolver for the outbound HTTP client that only ever hands back public
/// addresses.
///
/// `validate_url` runs before each request, but the HTTP client resolves the
/// name again when it connects. Filtering at that second lookup closes the
/// gap: whatever the name resolves to at connect time, a private, loopback or
/// link-local address is never dialled. URLs with an IP literal bypass DNS
/// entirely and are covered by `validate_url` alone.
pub struct PublicOnlyResolver;

impl reqwest::dns::Resolve for PublicOnlyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|addr| !is_private_ip(addr.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("'{host}' has no public address").into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Resolve a hostname and check all returned IPs against the private range deny list.
/// Returns the checked addresses.
async fn check_host_ips(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    // If the host is a raw IP address, check it directly
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(ip) {
            return Err(format!("Private/reserved IP address not allowed: {ip}"));
        }
        return Ok(vec![SocketAddr::new(ip, port)]);
    }

    // DNS resolution — reject the name if ANY returned address is private.
    // This alone does not stop DNS rebinding (the answer can change on the
    // next lookup); callers connect to the addresses returned here, or go
    // through `PublicOnlyResolver`.
    let addr_str = format!("{host}:{port}");
    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host(&addr_str)
        .await
        .map_err(|e| format!("DNS resolution failed for '{host}': {e}"))?
        .collect();

    if addrs.is_empty() {
        return Err(format!("DNS resolution returned no addresses for '{host}'"));
    }

    for addr in &addrs {
        if is_private_ip(addr.ip()) {
            return Err(format!(
                "Host '{host}' resolves to private/reserved IP: {}",
                addr.ip()
            ));
        }
    }

    Ok(addrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_private_ipv4() {
        assert!(is_private_ip("127.0.0.1".parse().unwrap()));
        assert!(is_private_ip("10.0.0.1".parse().unwrap()));
        assert!(is_private_ip("172.16.0.1".parse().unwrap()));
        assert!(is_private_ip("192.168.1.1".parse().unwrap()));
        assert!(is_private_ip("169.254.169.254".parse().unwrap()));
        assert!(is_private_ip("0.0.0.0".parse().unwrap()));
        assert!(is_private_ip("100.64.0.1".parse().unwrap())); // CGNAT
    }

    #[test]
    fn test_public_ipv4() {
        assert!(!is_private_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip("1.1.1.1".parse().unwrap()));
        assert!(!is_private_ip("203.0.113.1".parse().unwrap()));
    }

    #[test]
    fn test_private_ipv6() {
        assert!(is_private_ip("::1".parse().unwrap()));
        assert!(is_private_ip("fe80::1".parse().unwrap()));
        assert!(is_private_ip("fc00::1".parse().unwrap()));
        assert!(is_private_ip("::".parse().unwrap()));
    }

    #[test]
    fn test_ipv4_mapped_ipv6() {
        // ::ffff:127.0.0.1 must be treated as loopback
        assert!(is_private_ip("::ffff:127.0.0.1".parse().unwrap()));
        // ::ffff:10.0.0.1 must be treated as private
        assert!(is_private_ip("::ffff:10.0.0.1".parse().unwrap()));
        // ::ffff:192.168.1.1 must be treated as private
        assert!(is_private_ip("::ffff:192.168.1.1".parse().unwrap()));
        // ::ffff:8.8.8.8 is public
        assert!(!is_private_ip("::ffff:8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn test_public_ipv6() {
        assert!(!is_private_ip("2001:4860:4860::8888".parse().unwrap())); // Google DNS
    }

    #[tokio::test]
    async fn test_validate_url_bad_scheme() {
        let result = validate_url("ftp://example.com/file", &["http", "https"]).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("scheme"));
    }

    #[tokio::test]
    async fn test_validate_url_private_ip() {
        let result = validate_url("http://127.0.0.1:8080/admin", &["http", "https"]).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Private"));
    }

    #[tokio::test]
    async fn test_validate_url_aws_metadata() {
        let result = validate_url(
            "http://169.254.169.254/latest/meta-data/",
            &["http", "https"],
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_validate_host_private() {
        let result = validate_host("192.168.1.1:80").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn resolve_public_host_returns_checked_literal() {
        let addrs = resolve_public_host("8.8.8.8:53").await.unwrap();
        assert_eq!(addrs, vec!["8.8.8.8:53".parse::<SocketAddr>().unwrap()]);
        assert!(resolve_public_host("10.0.0.1:53").await.is_err());
    }

    #[tokio::test]
    async fn public_only_resolver_refuses_names_that_resolve_internally() {
        use reqwest::dns::Resolve;
        use std::str::FromStr;
        // `localhost` resolves to loopback only, so nothing public remains.
        let name = reqwest::dns::Name::from_str("localhost").unwrap();
        assert!(PublicOnlyResolver.resolve(name).await.is_err());
    }

    #[tokio::test]
    async fn test_validate_host_loopback() {
        let result = validate_host("127.0.0.1:5432").await;
        assert!(result.is_err());
    }
}
