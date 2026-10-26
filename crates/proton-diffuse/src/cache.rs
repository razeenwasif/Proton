//! What forge served last, kept in the project's `.proton/diffuse/`: its
//! node catalogue and its model lists.
//!
//! The editor needs the catalogue to draw a node and the model lists to fill
//! a dropdown, and forge is a desktop that is off as often as it is on. So
//! each time it answers, what it said is written down, and when it does not,
//! that is what the editor works from — graphs open, edit and save; only Run
//! waits. Written as the server sent it, whole, beside and renamed over.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalogue::{Catalogue, Models};
use crate::client::{Client, Health, HealthState};
use crate::doc::write_atomically;

const CATALOGUE: &str = "catalogue.json";
const MODELS: &str = "models.json";

/// What Proton knows of the server: as it answered now, or as it was kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseKnown")]
pub struct Known {
    pub health: Health,
    pub catalogue: Option<Catalogue>,
    pub models: Option<Models>,
    /// Whether the catalogue and models are what the server said just now,
    /// rather than what was kept from before.
    pub fresh: bool,
}

/// Look at the server — through `client`, or with none when there is no
/// key — and keep what it says in `dir`; or, when it is not serving, read
/// what was kept there.
pub fn look(client: Option<&Client>, dir: &Path) -> Known {
    let Some(client) = client else { return kept(Health::no_key(), dir) };
    let mut health = client.health();
    if health.state != HealthState::Serving {
        return kept(health, dir);
    }
    let answered = client.nodes().and_then(|catalogue| client.models().map(|models| (catalogue, models)));
    match answered {
        Ok((catalogue, models)) => {
            // Kept or not, it is what forge says now; a full disk is no
            // reason to stop editing.
            let _ = keep(dir, &catalogue, &models);
            Known { health, catalogue: Some(catalogue), models: Some(models), fresh: true }
        }
        Err(e) => {
            health.state = HealthState::Failing;
            health.message = e.describe();
            kept(health, dir)
        }
    }
}

/// What was kept in `dir`, if anything.
pub fn kept(health: Health, dir: &Path) -> Known {
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
    let catalogue = read(CATALOGUE).and_then(|text| Catalogue::parse(&text).ok());
    let models = read(MODELS).and_then(|text| serde_json::from_str(&text).ok());
    Known { health, catalogue, models, fresh: false }
}

/// Write down what the server said.
pub fn keep(dir: &Path, catalogue: &Catalogue, models: &Models) -> std::io::Result<()> {
    write_atomically(&dir.join(CATALOGUE), pretty(catalogue).as_bytes())?;
    write_atomically(&dir.join(MODELS), pretty(models).as_bytes())
}

/// Pretty JSON ending in a newline.
fn pretty<T: Serialize>(value: &T) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into());
    text.push('\n');
    text
}
