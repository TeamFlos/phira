use anyhow::Result;
use trust_dns_resolver::{
    config::{ResolverConfig, ResolverOpts},
    TokioAsyncResolver,
};

const SRV_PREFIX: &str = "_phira._tcp.";

/// Resolves a server address, attempting SRV resolution if no port is specified.
///
/// If the address contains a colon (`:`) indicating a port, it is returned as-is.
/// Otherwise, attempts to resolve an SRV record for `_phira._tcp.<domain>`.
/// If SRV resolution succeeds, returns the target host and port from the SRV record.
/// If SRV resolution fails, returns an error.
pub async fn resolve_server_address(address: &str) -> Result<String> {
    // If address already contains a port, return as-is
    if address.contains(':') {
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

/// Performs SRV DNS lookup for the given domain.
async fn resolve_srv(domain: &str) -> Result<String> {
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());

    let srv_name = format!("{}{}", SRV_PREFIX, domain);
    
    let lookup = resolver
        .srv_lookup(&srv_name)
        .await
        .map_err(|e| anyhow::anyhow!("SRV lookup failed: {}", e))?;

    // Get the first SRV record (typically the one with highest priority)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_address_with_port_returns_as_is() {
        let address = "example.com:12345";
        let result = resolve_server_address(address).await.unwrap();
        assert_eq!(result, "example.com:12345");
    }

    #[tokio::test]
    async fn test_address_without_port_requires_srv() {
        let address = "nonexistent-domain-for-testing.example";
        let result = resolve_server_address(address).await;
        assert!(result.is_err());
    }
}
