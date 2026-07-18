//! Read the account's setups from the sync API — the same authenticated pull
//! the apps do — so install can offer a pick list instead of a UUID prompt.

use lux_wire::{ListSetupsResponse, SetupRecord};

use crate::config::Endpoints;
use lux_engine::tls::webpki_pem_bundle;

pub async fn list(env: &Endpoints, id_token: &str) -> Result<Vec<SetupRecord>, String> {
    let certs = reqwest::Certificate::from_pem_bundle(webpki_pem_bundle())
        .map_err(|e| format!("webpki bundle: {e}"))?;
    let client = reqwest::Client::builder()
        .tls_certs_only(certs)
        .build()
        .map_err(|e| e.to_string())?;

    let base = env.sync_url.trim_end_matches('/');
    let resp = client
        .get(format!("{base}/{}", lux_wire::SETUPS_SEGMENT))
        .bearer_auth(id_token)
        .send()
        .await
        .map_err(|e| format!("sync api unreachable: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("sync api answered {}", resp.status()));
    }
    let list: ListSetupsResponse = resp.json().await.map_err(|e| format!("bad reply: {e}"))?;
    Ok(list
        .setups
        .into_iter()
        .filter(|setup| !setup.deleted)
        .collect())
}
