use anyhow::Result;
use once_cell::sync::Lazy;
use trust_dns_resolver::{
    config::{ResolverConfig, ResolverOpts},
    TokioAsyncResolver,
};

const SRV_PREFIX: &str = "_phira._tcp.";

/// Global DNS resolver that's reused across all lookups for efficiency
static RESOLVER: Lazy<TokioAsyncResolver> = Lazy::new(|| {
    TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default())
});

/// Resolves a server address, attempting SRV resolution if no port is specified.
///
/// Detection logic:
/// - IPv6 addresses in brackets with ports (e.g., `[::1]:8080`) are returned as-is
/// - IPv4 addresses with ports (e.g., `192.168.1.1:8080`) are returned as-is
/// - Domain names with ports (e.g., `example.com:12345`) are returned as-is
/// - Bare IPv6 addresses without brackets (e.g., `::1`) trigger SRV resolution
/// - Domain names without ports (e.g., `example.com`) trigger SRV resolution
///
/// If SRV resolution succeeds, returns the target host and port from the SRV record.
/// If SRV resolution fails, returns an error.
pub async fn resolve_server_address(address: &str) -> Result<String> {
    // If address contains a port (simple heuristic: last colon followed by digits),
    // or is an IPv6 address in brackets, return as-is
    if has_port(address) {
        return Ok(address.to_string());
    }

    // Attempt SRV resolution
    match resolve_srv(address).await {
        Ok(resolved) => Ok(resolved),
        Err(e) => {
            // SRV resolution failed, return error
            Err(anyhow::anyhow!(
                "Failed to resolve SRV record for '{}': {}. Please specify host:port explicitly.",
                address,
                e
            ))
        }
    }
}

/// Checks if an address appears to have a port specified.
/// Handles both IPv4:port and [IPv6]:port formats.
fn has_port(address: &str) -> bool {
    // Check for IPv6 with port: [::1]:8080
    if address.starts_with('[') {
        return address.contains("]:");
    }
    
    // For non-bracketed addresses, check if there's a colon followed by digits
    // IPv6 addresses without ports will have multiple colons or non-digit characters after colons
    if let Some(colon_pos) = address.rfind(':') {
        // Check if everything after the last colon is digits (port)
        let after_colon = &address[colon_pos + 1..];
        if after_colon.is_empty() {
            return false;
        }
        // If it's all digits and we only have one colon (or the part before has no colons),
        // it's likely host:port format
        if after_colon.chars().all(|c| c.is_ascii_digit()) {
            // Check if there's another colon before this one (would indicate IPv6)
            let before_colon = &address[..colon_pos];
            return !before_colon.contains(':');
        }
    }
    
    false
}

/// Performs SRV DNS lookup for the given domain.
/// SRV records are automatically returned by the DNS resolver in priority order.
async fn resolve_srv(domain: &str) -> Result<String> {
    let srv_name = format!("{}{}", SRV_PREFIX, domain);
    
    let lookup = RESOLVER
        .srv_lookup(&srv_name)
        .await
        .map_err(|e| anyhow::anyhow!("SRV lookup failed: {}", e))?;

    // Get the first SRV record - the DNS resolver returns records in priority order
    // (lowest priority value first), so we can simply take the first one
    let srv = lookup
        .iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No SRV records found"))?;

    let target = srv.target().to_string();
    let port = srv.port();

    // Remove trailing dot from target if present
    let target = target.trim_end_matches('.');

    Ok(format!("{}:{}", target, port))
}
