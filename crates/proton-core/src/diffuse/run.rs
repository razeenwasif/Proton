//! Runs: a flow's graph sent to forge, followed as it happens, its images
//! saved into the project.
//!
//! Each run has a worker of its own. It sends the graph, reads the run's
//! event stream line by line as forge works, fetches each image as it is
//! made and writes it where it belongs — a Save Image's into the flow's
//! outputs folder, numbered after those already there; a Preview Image's
//! into `.proton/diffuse/previews/`, out of the way — and tells the core
//! what happened. The core keeps what each run has said, for the Queue,
//! the graph's node states and the gallery, and polls the news in.
//!
//! forge queues runs itself, one at a time, so several sent at once each
//! wait their turn there; Proton only follows them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use proton_diffuse::client::NodeState;
use proton_diffuse::{Error as ClientError, Event, Graph, Seal};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Gateway;
use crate::settings::Settings;

/// A run, as Proton has followed it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DiffuseRun {
    /// Proton's number for it, this session.
    pub id: u32,
    /// forge's, once it is queued.
    pub run: Option<String>,
    /// The flow it ran, as the project names it.
    pub flow: String,
    pub state: RunState,
    /// Its place in forge's queue while it waits.
    pub position: Option<u32>,
    /// Each node's state, in the order forge reached them.
    pub nodes: Vec<NodeRun>,
    pub progress: Option<RunProgress>,
    /// The latest preview of the node at work, as a `data:` URL.
    pub preview: Option<String>,
    pub outputs: Vec<RunOutput>,
    /// Why it failed.
    pub message: Option<String>,
    /// Where it failed: each node forge refused or stopped at, and why.
    pub problems: Vec<RunProblem>,
    /// How long forge took, from its start to its end.
    pub seconds: Option<f64>,
    /// When it was sent, in milliseconds since 1970.
    #[ts(type = "number")]
    pub started: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseRunState")]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    /// On its way to forge.
    Sending,
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
    /// Its progress could not be followed: the stream dropped and could not
    /// be opened again, or forge no longer has it.
    Lost,
}

impl RunState {
    pub fn ended(self) -> bool {
        matches!(self, RunState::Done | RunState::Failed | RunState::Cancelled | RunState::Lost)
    }
}

/// What went wrong at a node — or, with none, with the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseRunProblem")]
pub struct RunProblem {
    pub node: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseNodeRun")]
pub struct NodeRun {
    pub id: String,
    pub state: NodeState,
    /// How long it took, once done.
    pub seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseRunProgress")]
pub struct RunProgress {
    pub node: String,
    pub step: u32,
    pub steps: u32,
}

/// An image a run made, saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "DiffuseRunOutput")]
pub struct RunOutput {
    /// The node that made it.
    pub node: String,
    /// Where it is, as the project names it.
    pub path: String,
    pub width: u32,
    pub height: u32,
    /// In the outputs folder (a Save Image's), or only a preview.
    pub kept: bool,
}

/// What a worker tells the core.
#[derive(Debug)]
pub(super) enum News {
    Queued {
        id: u32,
        run: String,
        position: u32,
    },
    Event {
        id: u32,
        event: Event,
    },
    Saved {
        id: u32,
        output: RunOutput,
    },
    Failed {
        id: u32,
        message: String,
    },
    /// forge refused the graph: what it said, and each node's trouble.
    Refused {
        id: u32,
        message: String,
        nodes: BTreeMap<String, String>,
    },
    /// The run's progress could no longer be followed.
    Lost {
        id: u32,
        message: String,
    },
}

/// How the core stops a run: the flag a worker looks at before it is
/// queued, and forge's id once it is.
#[derive(Debug, Clone, Default)]
pub(super) struct Control {
    pub cancel: Arc<AtomicBool>,
    pub run: Arc<Mutex<Option<String>>>,
}

/// What a worker needs.
pub(super) struct Job {
    pub id: u32,
    pub gateway: Gateway,
    pub person: Settings,
    pub graph: Graph,
    pub root: PathBuf,
    /// The flow's outputs folder, in the project.
    pub outputs: String,
    /// The output nodes: whether each keeps its images, and their prefix.
    pub saves: BTreeMap<String, (bool, String)>,
    /// The pictures a run sends first: each one's node and field, its file,
    /// and the mask painted over it, if one was.
    pub inputs: Vec<Picture>,
    pub control: Control,
    /// What the run's images are sealed with on disk, and its pictures read
    /// with; plain without.
    pub seal: Option<Seal>,
}

/// A picture to upload before the graph is sent.
#[derive(Debug, Clone)]
pub(super) struct Picture {
    pub node: String,
    pub field: String,
    pub file: PathBuf,
    pub mask: Option<PathBuf>,
}

/// A picture as forge takes it: the picture, and the mask — white where it
/// is painted — in its alpha, transparent where masked, as ComfyUI's Load
/// Image reads one. Without a mask, the file as it is.
pub(super) fn upload_bytes(picture: &Picture, seal: Option<&Seal>) -> Result<Vec<u8>, String> {
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = proton_diffuse::seal::read(&picture.file, seal).map_err(|e| format!("{}: {e}", name(&picture.file)))?;
    let Some(mask) = &picture.mask else { return Ok(bytes) };
    let (w, h, mut rgba) =
        proton_raster::file::decode_png(&bytes).map_err(|e| format!("{}: {e}", name(&picture.file)))?;
    let read = proton_diffuse::seal::read(mask, seal).map_err(|e| format!("{}: {e}", name(mask)))?;
    let (mw, mh, shade) = proton_raster::file::decode_png(&read).map_err(|e| format!("{}: {e}", name(mask)))?;
    if (mw, mh) != (w, h) {
        return Err(format!("the mask is {mw} × {mh} and the picture {w} × {h}: paint the mask again"));
    }
    for (pixel, painted) in rgba.chunks_exact_mut(4).zip(shade.chunks_exact(4)) {
        // How much is masked: the mask's brightness, as far as it is opaque.
        let masked = (painted[0] as u32 * painted[3] as u32 / 255) as u8;
        pixel[3] = pixel[3].min(255 - masked);
    }
    proton_raster::file::encode_png(w, h, &rgba)
}

/// Start a run's worker.
pub(super) fn start(job: Job, tell: Sender<News>) {
    std::thread::spawn(move || {
        let id = job.id;
        // Whatever happens to the worker, the run ends said: a worker that
        // stopped without a word would leave it running for ever.
        let followed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| follow(&job, &tell)));
        let message = match followed {
            Ok(Ok(())) => return,
            Ok(Err(message)) => message,
            Err(_) => "Proton's run worker stopped unexpectedly".to_string(),
        };
        let _ = tell.send(News::Failed { id, message });
    });
}

fn follow(job: &Job, tell: &Sender<News>) -> Result<(), String> {
    let id = job.id;
    let mut gateway = job.gateway.clone();
    let client =
        gateway.client(&job.person).ok_or("no NEXUS_KEY in your shell: export it in your profile, then look again")?;
    // Each picture first: forge keeps an upload a day, so a flow names a
    // file of its own and the run sends it, its mask in its alpha.
    let mut graph = job.graph.clone();
    for picture in &job.inputs {
        let bytes = upload_bytes(picture, job.seal.as_ref())?;
        let upload = client.upload(&bytes).map_err(|e| e.describe())?;
        if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == picture.node) {
            node.values.insert(picture.field.clone(), serde_json::Value::String(upload.id));
        }
    }
    let queued = match client.run(&graph) {
        Ok(queued) => queued,
        // Refused at the door: what forge said, and each node's trouble.
        Err(ClientError::Invalid { message, nodes }) => {
            let message = if message.is_empty() { "forge refused the graph".to_string() } else { message };
            let _ = tell.send(News::Refused { id, message, nodes });
            return Ok(());
        }
        Err(e) => return Err(e.describe()),
    };
    *job.control.run.lock().unwrap_or_else(|e| e.into_inner()) = Some(queued.run.clone());
    let _ = tell.send(News::Queued { id, run: queued.run.clone(), position: queued.position });
    // Stopped while it was on its way: stop it there too.
    if job.control.cancel.load(Ordering::Relaxed) {
        let _ = client.cancel(&queued.run);
    }
    // The stream replays the run from its start (forge keeps it a day), so
    // one that drops is opened again and what was heard already is passed
    // over: no event is taken twice, no image saved twice. One that cannot
    // be opened again leaves the run lost, never still running.
    let mut heard = 0usize;
    let mut tries = 0u32;
    loop {
        let events = match client.events(&queued.run) {
            Ok(events) => events,
            Err(ClientError::Server { status: 404, .. }) => {
                let _ = tell.send(News::Lost { id, message: "forge no longer has this run".into() });
                return Ok(());
            }
            Err(e) => {
                if !wait_to_retry(&mut tries, &job.control) {
                    let _ = tell.send(News::Lost {
                        id,
                        message: format!("the run's progress could not be followed: {}", e.describe()),
                    });
                    return Ok(());
                }
                continue;
            }
        };
        let mut index = 0usize;
        for event in events {
            let event = match event {
                Ok(event) => event,
                // Cut off: open it again.
                Err(ClientError::Lost(_)) => break,
                Err(e) => return Err(e.describe()),
            };
            index += 1;
            if index <= heard {
                continue;
            }
            heard = index;
            tries = 0;
            if let Event::Image { id: node, image, .. } = &event {
                let bytes = client.image(image).map_err(|e| e.describe())?;
                let (kept, prefix) = job.saves.get(node).cloned().unwrap_or((false, "image".into()));
                let output = save(&job.root, &job.outputs, node, image, kept, &prefix, &bytes, job.seal.as_ref())?;
                let _ = tell.send(News::Saved { id, output });
            }
            let ended = event.ends();
            if tell.send(News::Event { id, event }).is_err() || ended {
                return Ok(());
            }
        }
        if !wait_to_retry(&mut tries, &job.control) {
            let _ = tell.send(News::Lost {
                id,
                message: "the stream from forge closed before the run ended, and could not be opened again".into(),
            });
            return Ok(());
        }
    }
}

/// How many times a stream is opened again before the run is called lost.
const RETRIES: u32 = 3;

/// Wait before opening the stream again — longer each time — unless the run
/// was stopped or the tries are spent: whether to try.
fn wait_to_retry(tries: &mut u32, control: &Control) -> bool {
    *tries += 1;
    if *tries > RETRIES || control.cancel.load(Ordering::Relaxed) {
        return false;
    }
    std::thread::sleep(retry_wait(*tries));
    true
}

/// A test waits no time between tries; the app, a second more each time.
fn retry_wait(tries: u32) -> std::time::Duration {
    match std::env::var_os("PROTON_DIFFUSE_RETRY_MS") {
        Some(ms) => std::time::Duration::from_millis(ms.to_string_lossy().parse().unwrap_or(0)),
        None => std::time::Duration::from_secs(tries as u64),
    }
}

/// Write an image where it belongs: a kept one as `<prefix>_<nnnn>.png` in
/// the outputs folder, numbered after the last there; a preview under
/// `.proton/diffuse/previews/` by forge's name for it.
fn save(
    root: &Path,
    outputs: &str,
    node: &str,
    image: &str,
    kept: bool,
    prefix: &str,
    bytes: &[u8],
    seal: Option<&Seal>,
) -> Result<RunOutput, String> {
    let head = proton_diffuse::png::head(bytes).ok_or_else(|| format!("forge's image {image} is not a PNG"))?;
    let (folder, name) = match kept {
        true => {
            let folder = root.join(outputs);
            let prefix = plain(prefix);
            (folder.clone(), format!("{prefix}_{:04}.png", next_number(&folder, &prefix)))
        }
        false => (root.join(".proton").join("diffuse").join("previews"), format!("{}.png", plain(image))),
    };
    let path = folder.join(&name);
    proton_diffuse::seal::write(&path, bytes, seal)?;
    let rel = path.strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or(name);
    Ok(RunOutput { node: node.to_string(), path: rel, width: head.width, height: head.height, kept })
}

/// A name for a file: letters, digits, `-` and `_`.
fn plain(text: &str) -> String {
    let kept: String = text.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(60).collect();
    if kept.is_empty() {
        "image".into()
    } else {
        kept
    }
}

/// One more than the largest number after `<prefix>_` in the folder.
fn next_number(folder: &Path, prefix: &str) -> u32 {
    let lead = format!("{prefix}_");
    std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let name = proton_diffuse::seal::plain_name(&name).unwrap_or(&name);
                    name.strip_prefix(&lead)?.strip_suffix(".png")?.parse::<u32>().ok()
                })
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
        + 1
}

/// Milliseconds since 1970.
pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
