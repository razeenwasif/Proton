//! The LaTeX mode's core: the machine's TeX, as this workspace sees it.
//!
//! TeX is the machine's — TinyTeX, latexmk and texlab in `~/.proton/tex`,
//! made once with the person's say-so twice — and each workspace draws it
//! when it changes. What a project typesets, and where, comes next.

use std::sync::{Arc, Mutex, OnceLock};

use proton_lab::tex::{Bundle, TexEnv, TexState};

use crate::bus::{Event, NoticeLevel};
use crate::workspace::notice;

/// The machine's TeX, in `~/.proton/tex`, made the first time it is asked for.
static TEX: OnceLock<Arc<Mutex<TexEnv>>> = OnceLock::new();

fn shared_tex() -> Arc<Mutex<TexEnv>> {
    TEX.get_or_init(|| Arc::new(Mutex::new(TexEnv::at_home()))).clone()
}

/// Proton is closing: TeX, if it is being made, stops; what the making left
/// is taken away the next time Proton looks.
pub fn quit_tex() {
    if let Some(tex) = TEX.get() {
        tex.lock().unwrap_or_else(|e| e.into_inner()).quit();
    }
}

/// One workspace's LaTeX: the machine's TeX, and what this window last drew of it.
#[derive(Default)]
pub struct Tex {
    /// Given for a test; the machine's, otherwise, once first asked for.
    env: Option<Arc<Mutex<TexEnv>>>,
    /// The version of TeX's state last drawn.
    seen: Option<u64>,
}

impl std::fmt::Debug for Tex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tex").field("seen", &self.seen).finish()
    }
}

impl Tex {
    /// With this TeX rather than the machine's: for a test.
    pub fn with_env(env: TexEnv) -> Tex {
        Tex { env: Some(Arc::new(Mutex::new(env))), seen: None }
    }

    fn env(&mut self) -> Arc<Mutex<TexEnv>> {
        self.env.get_or_insert_with(shared_tex).clone()
    }

    /// TeX, looked at — its marker read, nothing started — when the mode is
    /// entered; drawn when it has changed since this window last drew it,
    /// or `always`.
    pub fn env_event(&mut self, always: bool) -> Vec<Event> {
        let env = self.env();
        let mut env = env.lock().unwrap_or_else(|e| e.into_inner());
        let stale = always && matches!(env.state(), TexState::Unknown | TexState::Missing { .. });
        if stale && !env.busy() {
            env.check();
        }
        if !always && self.seen == Some(env.version()) {
            return Vec::new();
        }
        self.seen = Some(env.version());
        vec![Event::TexEnv { env: env.state().clone() }]
    }

    /// Do something to TeX, and draw it.
    pub fn env_do(&mut self, act: impl FnOnce(&mut TexEnv) -> Result<(), String>) -> Vec<Event> {
        let done = {
            let env = self.env();
            let mut env = env.lock().unwrap_or_else(|e| e.into_inner());
            act(&mut env)
        };
        let mut events = self.env_event(true);
        if let Err(why) = done {
            events.push(notice(why, NoticeLevel::Warn));
        }
        events
    }

    /// The first say-so: plan the bundle asked for.
    pub fn plan(&mut self, bundle: Bundle) -> Vec<Event> {
        self.env_do(|env| env.plan(bundle))
    }

    /// What has happened since the last beat: TeX being planned, made or
    /// taken away, drawn when it changed.
    pub fn poll(&mut self) -> Vec<Event> {
        let Some(env) = self.env.clone() else { return Vec::new() };
        {
            let mut env = env.lock().unwrap_or_else(|e| e.into_inner());
            if !env.busy() {
                drop(env);
                return self.env_event(false);
            }
            env.poll();
        }
        self.env_event(false)
    }
}
