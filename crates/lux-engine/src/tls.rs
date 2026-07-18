//! TLS trust glue shared by every outbound connection.

use std::sync::OnceLock;

/// The Mozilla CA set (webpki) as a single PEM bundle, built once.
///
/// Platform-native root stores are unreliable across lux's targets: iOS apps
/// can't read the system CA store at all (rustls parses zero roots and the
/// AWS SDK aborts), and a headless Linux box may not have ca-certificates
/// installed. Bundling the webpki roots makes TLS verification identical on
/// every platform.
pub fn webpki_pem_bundle() -> &'static [u8] {
    static BUNDLE: OnceLock<Vec<u8>> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        let mut pem = Vec::new();
        for cert in webpki_root_certs::TLS_SERVER_ROOT_CERTS {
            pem.extend_from_slice(
                pem::encode(&pem::Pem::new("CERTIFICATE", cert.as_ref().to_vec())).as_bytes(),
            );
        }
        pem
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundle_is_nonempty_pem() {
        let bundle = super::webpki_pem_bundle();
        assert!(bundle.starts_with(b"-----BEGIN CERTIFICATE-----"));
        assert!(bundle.len() > 10_000);
    }
}
