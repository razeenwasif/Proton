//! Diffuse in the core: where the gateway is, and the key to it.
//!
//! The flow format, the client and what forge served last are
//! `proton-diffuse`'s; what is the core's is the person. Their gateway's
//! addresses are their own settings — `diffuse.gateway` at home and
//! `diffuse.gatewayAway` over Tailscale, in `~/.config/proton/settings.json`
//! — since atlas is where they live, not where a project does. The key is
//! `NEXUS_KEY`: Proton's own environment, then the login shell's, as Fish's
//! is found. It is looked for once and kept in memory only, and looked for
//! again when the person asks — after they have just exported it.

use std::path::{Path, PathBuf};

use proton_diffuse::{Client, Key, Known};

use crate::settings::Settings;

/// The variable the gateway key is kept in.
pub const KEY_NAME: &str = "NEXUS_KEY";

/// The gateway at home: Nexus's coordinator on atlas, the Mac mini.
pub const HOME: &str = "http://atlas:7420/img/v1";

/// The gateway's addresses, in the order they are tried: home, then away
/// when one is set. Only `http` and `https` addresses are taken.
pub fn gateways(person: &Settings) -> Vec<String> {
    let home = person.string("diffuse.gateway", HOME);
    let away = person.string("diffuse.gatewayAway", "");
    [home, away]
        .into_iter()
        .map(|address| address.trim().trim_end_matches('/').to_string())
        .filter(|address| address.starts_with("http://") || address.starts_with("https://"))
        .collect()
}

/// Where a project keeps what forge served last.
pub fn kept_in(root: &Path) -> PathBuf {
    root.join(".proton").join("diffuse")
}

/// The key to the gateway, and the client it makes.
#[derive(Default)]
pub struct Gateway {
    /// The key once looked for: whether it was found.
    found: Option<Option<Key>>,
}

impl Gateway {
    /// The key: Proton's environment, then the login shell's.
    pub fn key(&mut self) -> Option<Key> {
        self.found
            .get_or_insert_with(|| {
                let own = std::env::var(KEY_NAME).ok().and_then(|v| Key::new(&v));
                own.or_else(|| {
                    let vars = proton_claude::process::login_vars(&[KEY_NAME]);
                    vars.get(KEY_NAME).and_then(|v| Key::new(v))
                })
            })
            .clone()
    }

    /// Look for the key again, as when it has just been exported.
    pub fn forget(&mut self) {
        self.found = None;
    }

    /// A client for the person's gateway, when there is a key.
    pub fn client(&mut self, person: &Settings) -> Option<Client> {
        let key = self.key()?;
        Some(Client::new(gateways(person), key))
    }

    /// How forge is, and its catalogue and models — just now, or as the
    /// project kept them. It asks over the network: call it off the lock.
    pub fn look(&mut self, root: &Path, person: &Settings) -> Known {
        let client = self.client(person);
        proton_diffuse::cache::look(client.as_ref(), &kept_in(root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn home_is_atlas_unless_the_person_says_otherwise() {
        assert_eq!(gateways(&Settings::saying(json!({}))), [HOME]);
        let both = Settings::saying(json!({"diffuse": {
            "gateway": "http://192.168.68.10:7420/img/v1/",
            "gatewayAway": " https://atlas.example.ts.net/img/v1 "
        }}));
        assert_eq!(gateways(&both), ["http://192.168.68.10:7420/img/v1", "https://atlas.example.ts.net/img/v1"]);
        // Nothing that is not a web address is tried.
        let odd = Settings::saying(json!({"diffuse": {"gateway": "atlas:7420", "gatewayAway": "file:///etc"}}));
        assert!(gateways(&odd).is_empty());
    }

    #[test]
    fn a_project_keeps_forges_answers_in_its_own_proton_folder() {
        assert_eq!(kept_in(Path::new("/p")), Path::new("/p/.proton/diffuse"));
    }
}
