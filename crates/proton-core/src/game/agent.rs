//! The agent's game tools: the open game read, edited as the person edits it,
//! checked, and played — headless, by the same simulation the window plays,
//! so "can the player still reach the ledge?" is answered by running it.
//!
//! An edit is the person's kind of edit: one step of their undo, held until
//! they save, on their screen at once. A run is reported in words (where the
//! player went, where it was hurt, which rooms it reached) and drawn over
//! the room for the person to see; when the game has unsaved changes the
//! same inputs are run on the saved version too, so an edit that breaks a
//! route shows as the route that worked before it.

use std::fmt::Write as _;

use proton_game::project::{Project, Target};
use proton_game::sim::world::{fx, input, SHIFT};
use proton_game::sim::{Happened, Sim, World};
use proton_game::Op;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::Game;
use crate::bus::Event;

/// The tools answered here, by name.
pub const TOOLS: [&str; 5] = ["game_info", "game_edit", "game_check", "game_play", "game_import"];

/// The most text one answer carries.
const BUDGET: usize = 60_000;
/// What `@game` sends of the room on screen, at most.
const ABOUT: usize = 12_000;
/// The longest run the agent may play: ten minutes at sixty ticks a second.
const MOST_TICKS: u32 = 36_000;
/// How many places along a run the answer lists.
const SAMPLES: usize = 40;

/// Where one run went in one room, for the stage to draw over it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GamePathPart {
    pub room: String,
    /// The player's feet, in pixels of the room, a few ticks apart.
    pub points: Vec<[i32; 2]>,
    /// Where it was hurt and sent back.
    pub hurt: Vec<[i32; 2]>,
}

/// `text` cut to at most `most` bytes, on a character's edge.
fn cut(text: &str, most: usize) -> &str {
    if text.len() <= most {
        return text;
    }
    let mut end = most;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn px(v: i32) -> i32 {
    v >> SHIFT
}

/// The start of a run: a room by its id (else where the game starts), a
/// place in it in pixels (else its first spawn), and abilities by their ids.
pub(super) fn start_of(world: &World, room: Option<&str>, at: Option<[i32; 2]>, abilities: &[String]) -> Result<(u16, [i32; 2], u32), String> {
    let mut bits = 0;
    for id in abilities {
        match world.abilities.iter().position(|a| a == id) {
            Some(i) => bits |= 1 << i,
            None => return Err(format!("the game has no ability {id}; it has {}", list(&world.abilities))),
        }
    }
    let (index, place) = match room {
        Some(id) => {
            let index = world.room(id).ok_or_else(|| format!("there is no room {id}"))?;
            let there = &world.rooms[index as usize];
            let place = match at {
                Some(p) => [fx(p[0] as f64), fx(p[1] as f64)],
                None => there.spawns.first().map(|s| s.1).unwrap_or([fx(64.0), fx(64.0)]),
            };
            (index, place)
        }
        None => match at {
            Some(p) => (world.start.0, [fx(p[0] as f64), fx(p[1] as f64)]),
            None => world.start,
        },
    };
    Ok((index, place, bits))
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "none".into()
    } else {
        items.join(", ")
    }
}

/// A run played to its end, as the answer and the stage need it.
struct Played {
    ticks: u32,
    hash: u64,
    end_room: String,
    end: [i32; 2],
    grounded: bool,
    /// Tick, room, where.
    hurt: Vec<(u32, String, [i32; 2])>,
    /// Tick and room, each time one was entered.
    entered: Vec<(u32, String)>,
    gained: Vec<(u32, String)>,
    jumps: u32,
    dashes: u32,
    /// The cues the animation said, and how many times each.
    cues: Vec<(String, u32)>,
    swings: u32,
    hits: u32,
    /// Tick and the actor's id, each thing beaten.
    beaten: Vec<(u32, String)>,
    /// Tick and room, each death.
    died: Vec<(u32, String)>,
    rested: u32,
    flags: Vec<(u32, String)>,
    shots: u32,
    /// Health at the end, and the most: none when nothing kills the player.
    health: Option<(i32, i32)>,
    samples: Vec<(u32, String, [i32; 2], bool)>,
    parts: Vec<GamePathPart>,
}

fn play_out(world: World, start: (u16, [i32; 2], u32), inputs: &[(u32, u32)]) -> Played {
    let names: Vec<String> = world.rooms.iter().map(|r| r.id.clone()).collect();
    let abilities = world.abilities.clone();
    let cue_names = world.cues.clone();
    let actor_names: Vec<String> = world.actors.iter().map(|a| a.id.clone()).collect();
    let flag_names = world.flags.clone();
    let mut sim = Sim::at(world, start.0, start.1, start.2);
    let total: u32 = inputs.iter().map(|(_, n)| n).sum();
    let every = (total / SAMPLES as u32).max(1);
    let feet = |sim: &Sim| [px(sim.state.pos[0]), px(sim.state.pos[1])];
    let room_of = |sim: &Sim| names.get(sim.state.room as usize).cloned().unwrap_or_default();
    let mut out = Played {
        ticks: 0,
        hash: 0,
        end_room: String::new(),
        end: [0, 0],
        grounded: false,
        hurt: Vec::new(),
        entered: Vec::new(),
        gained: Vec::new(),
        jumps: 0,
        dashes: 0,
        cues: Vec::new(),
        swings: 0,
        hits: 0,
        beaten: Vec::new(),
        died: Vec::new(),
        rested: 0,
        flags: Vec::new(),
        shots: 0,
        health: None,
        samples: vec![(0, room_of(&sim), feet(&sim), sim.state.grounded)],
        parts: vec![GamePathPart { room: room_of(&sim), points: vec![feet(&sim)], hurt: Vec::new() }],
    };
    for &(held, ticks) in inputs {
        for _ in 0..ticks {
            let before = feet(&sim);
            sim.step(held);
            let tick = sim.state.tick;
            let room = room_of(&sim);
            for what in sim.happened() {
                match what {
                    Happened::Jumped => out.jumps += 1,
                    Happened::Dashed => out.dashes += 1,
                    Happened::Landed => {}
                    Happened::Hurt => {
                        out.hurt.push((tick, room.clone(), before));
                        if let Some(part) = out.parts.last_mut() {
                            part.hurt.push(before);
                        }
                    }
                    Happened::Entered(_) => out.entered.push((tick, room.clone())),
                    Happened::Gained(bits) => {
                        for (i, id) in abilities.iter().enumerate() {
                            if bits & (1 << i) != 0 {
                                out.gained.push((tick, id.clone()));
                            }
                        }
                    }
                    Happened::Cue(c) => {
                        let name = cue_names.get(c as usize).cloned().unwrap_or_default();
                        match out.cues.iter_mut().find(|(n, _)| *n == name) {
                            Some((_, count)) => *count += 1,
                            None => out.cues.push((name, 1)),
                        }
                    }
                    Happened::Swung => out.swings += 1,
                    Happened::Hit => out.hits += 1,
                    Happened::Killed(a) => out.beaten.push((tick, actor_names.get(a as usize).cloned().unwrap_or_default())),
                    Happened::Died => out.died.push((tick, room.clone())),
                    Happened::Rested => out.rested += 1,
                    Happened::Flag(f) => out.flags.push((tick, flag_names.get(f as usize).cloned().unwrap_or_default())),
                    Happened::Shot => out.shots += 1,
                }
            }
            if out.parts.last().map(|p| p.room != room).unwrap_or(true) {
                out.parts.push(GamePathPart { room: room.clone(), points: Vec::new(), hurt: Vec::new() });
            }
            if tick % 2 == 0 {
                if let Some(part) = out.parts.last_mut() {
                    part.points.push(feet(&sim));
                }
            }
            if tick % every == 0 {
                out.samples.push((tick, room, feet(&sim), sim.state.grounded));
            }
        }
    }
    out.ticks = sim.state.tick;
    out.hash = sim.hash();
    out.end_room = room_of(&sim);
    out.end = feet(&sim);
    out.grounded = sim.state.grounded;
    out.health = (sim.world.fight.health > 0).then_some((sim.state.health, sim.world.fight.health));
    if let Some(part) = out.parts.last_mut() {
        part.points.push(out.end);
    }
    out
}

/// One run, in a few lines.
fn summary(p: &Played, tick: u32) -> String {
    let seconds = |t: u32| t as f64 / tick.max(1) as f64;
    let mut out = format!(
        "Ended in {} at ({}, {}), {}, after {} ticks ({:.1} s). {} jump{}, {} dash{}.",
        p.end_room,
        p.end[0],
        p.end[1],
        if p.grounded { "on the ground" } else { "in the air" },
        p.ticks,
        seconds(p.ticks),
        p.jumps,
        if p.jumps == 1 { "" } else { "s" },
        p.dashes,
        if p.dashes == 1 { "" } else { "es" },
    );
    if p.hurt.is_empty() {
        out.push_str(" Never hurt.");
    } else {
        let _ = write!(out, " Hurt {} time{}: ", p.hurt.len(), if p.hurt.len() == 1 { "" } else { "s" });
        let places: Vec<String> = p.hurt.iter().take(10).map(|(t, room, at)| format!("tick {t} in {room} at ({}, {})", at[0], at[1])).collect();
        out.push_str(&places.join("; "));
        out.push('.');
    }
    if !p.entered.is_empty() {
        let rooms: Vec<String> = p.entered.iter().map(|(t, room)| format!("{room} at tick {t}")).collect();
        let _ = write!(out, " Went into {}.", rooms.join(", then "));
    }
    if !p.gained.is_empty() {
        let got: Vec<String> = p.gained.iter().map(|(t, id)| format!("{id} at tick {t}")).collect();
        let _ = write!(out, " Gained {}.", got.join(", "));
    }
    if !p.cues.is_empty() {
        let said: Vec<String> = p.cues.iter().map(|(name, n)| format!("{name} ×{n}")).collect();
        let _ = write!(out, " Cues said: {}.", said.join(", "));
    }
    if p.swings > 0 {
        let _ = write!(out, " Swung {} time{}, landing {}.", p.swings, if p.swings == 1 { "" } else { "s" }, p.hits);
    }
    if !p.beaten.is_empty() {
        let beaten: Vec<String> = p.beaten.iter().map(|(t, id)| format!("{id} at tick {t}")).collect();
        let _ = write!(out, " Beat {}.", beaten.join(", "));
    }
    if p.shots > 0 {
        let _ = write!(out, " {} shot{} fired at the player.", p.shots, if p.shots == 1 { " was" } else { "s were" });
    }
    if !p.died.is_empty() {
        let died: Vec<String> = p.died.iter().map(|(t, room)| format!("tick {t} in {room}")).collect();
        let _ = write!(out, " Died {}.", died.join(", "));
    }
    if p.rested > 0 {
        let _ = write!(out, " Rested at a bench {} time{}.", p.rested, if p.rested == 1 { "" } else { "s" });
    }
    if !p.flags.is_empty() {
        let set: Vec<String> = p.flags.iter().map(|(t, f)| format!("{f} at tick {t}")).collect();
        let _ = write!(out, " Flags set: {}.", set.join(", "));
    }
    if let Some((left, most)) = p.health {
        let _ = write!(out, " Health {left} of {most}.");
    }
    out
}

/// The inputs a run holds, from the tool's `inputs`.
fn inputs_of(value: Option<&Value>) -> Result<Vec<(u32, u32)>, String> {
    let names = || input::ACTIONS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
    let steps = value.and_then(Value::as_array).ok_or("game_play: `inputs` is required: a list of {\"hold\": [actions], \"ticks\": n}")?;
    let mut out = Vec::new();
    let mut total = 0u32;
    for step in steps {
        let ticks = step.get("ticks").and_then(Value::as_u64).ok_or("game_play: each input needs `ticks`, a whole number of ticks to hold it")? as u32;
        let mut held = 0;
        for action in step.get("hold").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            let name = action.as_str().unwrap_or("");
            match input::ACTIONS.iter().find(|(n, _)| *n == name) {
                Some((_, bit)) => held |= bit,
                None => return Err(format!("game_play: no action {name:?}; the actions are {}", names())),
            }
        }
        total = total.saturating_add(ticks);
        if total > MOST_TICKS {
            return Err(format!("game_play: at most {MOST_TICKS} ticks in one run"));
        }
        if ticks > 0 {
            out.push((held, ticks));
        }
    }
    if out.is_empty() {
        return Err("game_play: the inputs hold nothing for any ticks".into());
    }
    Ok(out)
}

impl Game {
    /// Answer one of the agent's game tools, with what the window is sent.
    pub fn tool(&mut self, name: &str, input: &Value) -> Result<String, String> {
        if self.project.is_none() {
            return Err("No game is open in Game mode. The person opens one from its home page (⌃8).".into());
        }
        match name {
            "game_info" => self.tool_info(input),
            "game_edit" => self.tool_edit(input),
            "game_check" => Ok(self.tool_check()),
            "game_play" => self.tool_play(input),
            "game_import" => {
                let source = input.get("source").and_then(Value::as_str).ok_or("game_import: `source` is required: a .psd, .png, .aseprite or sprite sheet .json in the game's folder")?;
                let room = input.get("room").and_then(Value::as_str);
                let scale = input.get("scale").and_then(Value::as_f64);
                let normals = input.get("normals").and_then(Value::as_bool).unwrap_or(false);
                let done = self.import_art(source, room, scale, normals)?;
                self.pending.extend(done.events);
                let mut out = format!(
                    "{}. The images are written into the game's art folder now; the room's change is held until the person saves, and ⌘Z undoes it.",
                    done.said
                );
                if !done.notes.is_empty() {
                    out.push_str("\nNotes:\n- ");
                    out.push_str(&done.notes.join("\n- "));
                }
                Ok(out)
            }
            _ => Err(format!("{name}: not a game tool")),
        }
    }

    /// What the agent's last call changed on screen, for the window.
    pub fn take_pending(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    fn tool_info(&self, input: &Value) -> Result<String, String> {
        let project = self.project.as_ref().ok_or("no game is open")?;
        let Some(which) = input.get("document").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
            return Ok(self.summary());
        };
        let target = Target::parse(which).ok_or_else(|| format!("game_info: {which:?} is not a document: game, world, room:<id> or prefab:<id>"))?;
        let (rel, text) = match &target {
            Target::Game => (project.game.rel().to_string(), project.game.text()),
            Target::World => (project.world.rel().to_string(), project.world.text()),
            Target::Room(id) => {
                let doc = project.room(id).ok_or_else(|| format!("there is no room {id}"))?;
                (doc.rel().to_string(), doc.text())
            }
            Target::Prefab(id) => {
                let doc = project.prefab(id).ok_or_else(|| format!("there is no prefab {id}"))?;
                (doc.rel().to_string(), doc.text())
            }
        };
        let unsaved = project.unsaved().contains(&target);
        let mut out = format!("{rel}{}:\n", if unsaved { " (with changes not saved)" } else { "" });
        out.push_str(cut(&text, BUDGET));
        if text.len() > BUDGET {
            let _ = write!(out, "\n… cut at {BUDGET} of {} bytes.", text.len());
        }
        Ok(out)
    }

    /// The game in a page: what `game_info` says with no document, and the
    /// head of what `@game` sends.
    fn summary(&self) -> String {
        let Some(project) = &self.project else { return "No game is open in Game mode.".into() };
        let game = project.game.get();
        let world = project.world.get();
        let c = &game.controller;
        let mut out = String::new();
        let _ = writeln!(out, "{} — a {:?} game in {} (game.json, world.json, rooms/, prefabs/).", game.name, game.niche, self.shown);
        let _ = writeln!(
            out,
            "View {}×{} px, focal length {}, {} ticks a second. Run {} px/s; jump {} px high, {} s to the top; coyote {} ticks, buffer {} ticks; {} air jump{}.",
            game.view.width,
            game.view.height,
            game.view.focal,
            game.physics.tick,
            c.run,
            c.jump.height,
            c.jump.apex,
            c.jump.coyote,
            c.jump.buffer,
            c.air_jumps,
            if c.air_jumps == 1 { "" } else { "s" },
        );
        let abilities: Vec<String> = game.abilities.iter().map(|a| if a.name.is_empty() { a.id.clone() } else { format!("{} ({})", a.id, a.name) }).collect();
        let _ = writeln!(out, "Abilities: {}.", list(&abilities));
        let _ = writeln!(out, "Starts in room {} at spawn {}.", game.start.room, game.start.spawn);
        let _ = writeln!(out, "Rooms:");
        for doc in &project.rooms {
            let room = doc.get();
            let area = world.rooms.iter().find(|r| r.id == room.id).map(|r| r.area.as_str()).unwrap_or("");
            let mut flags = Vec::new();
            if self.focus.as_deref() == Some(room.id.as_str()) {
                flags.push("on screen");
            } else if self.open.contains(&room.id) {
                flags.push("open");
            }
            if doc.is_dirty() {
                flags.push("not saved");
            }
            let entities: Vec<String> = room.entities.iter().map(|e| format!("{} ({})", e.id, e.prefab)).collect();
            let _ = writeln!(
                out,
                "- {} in {}: {}×{} px, {} planes, {} collision lines, spawns {}; entities {}{}",
                room.id,
                if area.is_empty() { "no area" } else { area },
                room.size[0],
                room.size[1],
                room.planes.len(),
                room.collision.len(),
                list(&room.spawns.iter().map(|s| s.id.clone()).collect::<Vec<_>>()),
                list(&entities),
                if flags.is_empty() { String::new() } else { format!(" [{}]", flags.join(", ")) },
            );
        }
        let prefabs: Vec<String> = project
            .prefabs
            .iter()
            .map(|p| {
                let p = p.get();
                let health = p.health.map(|h| format!(", health {h}")).unwrap_or_default();
                format!("{} ({}{health})", p.id, format!("{:?}", p.behaviour.kind).to_lowercase())
            })
            .collect();
        let _ = writeln!(out, "Prefabs: {}.", list(&prefabs));
        let k = &game.combat;
        let _ = writeln!(
            out,
            "Combat: a swing does {} for {} frames then rests {}, reaching {}×{} px; hitstop {} frames; hurt, the player is spared {} frames and stunned {}. Entities of a prefab may carry props patrol [from, to], facing (true: left) and flag (beating it sets the flag for good); a trigger (rect, props.flag) sets a flag; an ability_gate's props.flag keeps it shut until set; a save_point is a bench rested at with up.",
            k.damage, k.frames, k.recovery, k.reach[0], k.reach[1], k.hitstop, k.invincible, k.stun
        );
        let assets = super::assets(project.root());
        let art: Vec<String> = assets.iter().filter(|a| a.to_ascii_lowercase().ends_with(".psd")).cloned().collect();
        if !art.is_empty() {
            let _ = writeln!(out, "Layered art in its folder, for game_import: {}.", art.join(", "));
        }
        let to_animate: Vec<String> = assets
            .iter()
            .filter(|a| {
                let l = a.to_ascii_lowercase();
                l.ends_with(".aseprite") || l.ends_with(".ase") || (l.ends_with(".json") && !l.ends_with(".anim.json") && !l.ends_with(".skel.json") && !l.ends_with(".img.json"))
            })
            .cloned()
            .collect();
        if !to_animate.is_empty() {
            let _ = writeln!(out, "Animations to bring in, for game_import: {}.", to_animate.join(", "));
        }
        for rel in assets.iter().filter(|a| a.ends_with(".anim.json")) {
            let Ok(anim) = std::fs::read_to_string(project.root().join(rel)).map_err(|e| e.to_string()).and_then(|t| proton_game::doc::parse::<proton_game::format::Anim>(rel, &t)) else { continue };
            let users: Vec<&str> = project.prefabs.iter().map(|p| p.get()).filter(|p| p.sprite.as_deref() == Some(rel.as_str())).map(|p| p.id.as_str()).collect();
            let _ = writeln!(
                out,
                "Animation {rel}: {} frames of {}×{}, clips {}{}{}; {}.",
                anim.frames.len(),
                anim.size[0],
                anim.size[1],
                anim.clips.iter().map(|c| format!("{} ({}–{}, {:?})", c.name, c.from, c.to, c.direction).to_lowercase()).collect::<Vec<_>>().join(", "),
                if anim.boxes.is_empty() { String::new() } else { format!("; boxes {}", anim.boxes.iter().map(|b| format!("{} ({:?})", b.name, b.kind).to_lowercase()).collect::<Vec<_>>().join(", ")) },
                if anim.cues.is_empty() { String::new() } else { format!("; cues {}", anim.cues.iter().map(|c| format!("{} at frame {}", c.name, c.frame)).collect::<Vec<_>>().join(", ")) },
                if users.is_empty() { "no prefab plays it: set a prefab's sprite to it with game_edit".to_string() } else { format!("played by {}", users.join(", ")) },
            );
        }
        for rel in assets.iter().filter(|a| a.ends_with(".skel.json")) {
            let Ok(skel) = std::fs::read_to_string(project.root().join(rel)).map_err(|e| e.to_string()).and_then(|t| proton_game::doc::parse::<proton_game::format::Skel>(rel, &t)) else { continue };
            let users: Vec<&str> = project.prefabs.iter().map(|p| p.get()).filter(|p| p.sprite.as_deref() == Some(rel.as_str())).map(|p| p.id.as_str()).collect();
            let _ = writeln!(
                out,
                "Skeleton {rel}: {} bones, clips {}{}; {}.",
                skel.bones.len(),
                skel.clips.iter().map(|c| format!("{} ({} frames at {} a second{})", c.name, c.frames, skel.fps, if c.repeat == 0 { ", looping".to_string() } else { format!(", {}×", c.repeat) })).collect::<Vec<_>>().join(", "),
                if skel.boxes.is_empty() { String::new() } else { format!("; boxes {}", skel.boxes.iter().map(|b| format!("{} ({:?})", b.name, b.kind).to_lowercase()).collect::<Vec<_>>().join(", ")) },
                if users.is_empty() { "no prefab plays it: set a prefab's sprite to it with game_edit".to_string() } else { format!("played by {}", users.join(", ")) },
            );
        }
        for imported in self.imports.sources.iter().filter(|s| s.anim.is_empty()) {
            let _ = writeln!(
                out,
                "Imported {} into {}: {} plane{} and {} image{} on the play plane.",
                imported.source,
                imported.room,
                imported.planes.len(),
                if imported.planes.len() == 1 { "" } else { "s" },
                imported.items.len(),
                if imported.items.len() == 1 { "" } else { "s" },
            );
        }
        let unsaved = project.unsaved_files();
        if !unsaved.is_empty() {
            let _ = writeln!(out, "Not saved: {}.", unsaved.join(", "));
        }
        let problems = project.problems();
        if !problems.is_empty() {
            let _ = writeln!(out, "Problems: {}.", problems.join("; "));
        }
        if !self.recordings.is_empty() {
            let runs: Vec<String> = self.recordings.iter().map(|r| format!("{} ({}, {} ticks)", r.id, r.room, r.ticks)).collect();
            let _ = writeln!(out, "Runs the person played (game_play can replay one by its number): {}.", runs.join(", "));
        }
        out
    }

    /// What `@game` sends: the game, and the room on screen as it is now.
    pub fn about(&self) -> String {
        let mut out = self.summary();
        let room = self.project.as_ref().zip(self.focus.as_deref()).and_then(|(p, id)| p.room(id));
        if let Some(doc) = room {
            let text = doc.text();
            let _ = write!(out, "\nThe room on screen, {} ({}):\n{}", doc.get().id, doc.rel(), cut(&text, ABOUT));
            if text.len() > ABOUT {
                out.push_str("\n… cut; game_info with its document gives all of it.");
            }
        }
        out
    }

    fn tool_edit(&mut self, input: &Value) -> Result<String, String> {
        let which = input.get("document").and_then(Value::as_str).ok_or("game_edit: `document` is required: game, world, room:<id> or prefab:<id>")?;
        let target = Target::parse(which).ok_or_else(|| format!("game_edit: {which:?} is not a document: game, world, room:<id> or prefab:<id>"))?;
        let ops: Vec<Op> = serde_json::from_value(input.get("ops").cloned().unwrap_or(Value::Null))
            .map_err(|e| format!("game_edit: `ops` is a list of {{\"op\": \"set\"|\"insert\"|\"remove\"|\"move\", \"path\": [...], …}}: {e}"))?;
        if ops.is_empty() {
            return Err("game_edit: no ops to apply".into());
        }
        let project = self.project.as_mut().ok_or("no game is open")?;
        let before = project.problems();
        match project.apply(&target, &ops) {
            Ok(false) => return Ok(format!("{which} already says that: nothing changed.")),
            Ok(true) => {}
            Err(why) => return Err(format!("game_edit refused the whole batch, and nothing changed: {why}")),
        }
        self.done.push(target.clone());
        self.undone.clear();
        let events = self.changed(&target);
        self.pending.extend(events);
        let now = self.project.as_ref().map(Project::problems).unwrap_or_default();
        let new: Vec<&String> = now.iter().filter(|p| !before.contains(p)).collect();
        let mut out = format!(
            "Changed {which} ({} op{}). The change is held until the person saves: it is on their screen now and ⌘Z undoes it, so say what you changed rather than that a file was written.",
            ops.len(),
            if ops.len() == 1 { "" } else { "s" }
        );
        if !new.is_empty() {
            let _ = write!(out, " It leaves new problems: {}.", new.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("; "));
        }
        Ok(out)
    }

    fn tool_check(&self) -> String {
        let Some(project) = &self.project else { return "No game is open.".into() };
        let mut problems = project.problems();
        for doc in &project.rooms {
            let room = doc.get();
            if room.collision.is_empty() {
                problems.push(format!("room {} has no collision: the player falls through it", room.id));
            }
            if room.spawns.is_empty() {
                problems.push(format!("room {} has no spawn: nothing can arrive in it", room.id));
            }
        }
        if let Err(why) = World::cook(project) {
            problems.push(format!("the game cannot be played as it is: {why}"));
        }
        if problems.is_empty() {
            "Nothing wrong found: every door leads to a spawn, every gate and pickup names an ability the game has, every entity is built in or a prefab, and the game cooks into a world that plays.".into()
        } else {
            format!("{} problem{}:\n- {}", problems.len(), if problems.len() == 1 { "" } else { "s" }, problems.join("\n- "))
        }
    }

    fn tool_play(&mut self, input: &Value) -> Result<String, String> {
        let project = self.project.as_ref().ok_or("no game is open")?;
        let world = World::cook(project).map_err(|why| format!("the game cannot be played as it is: {why}"))?;
        let tick = world.tick;
        let room = input.get("room").and_then(Value::as_str);
        let at = input.get("at").and_then(Value::as_array).and_then(|a| Some([a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32]));
        let abilities: Vec<String> = input
            .get("abilities")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        let kept = input.get("recording").and_then(Value::as_u64).map(|id| id as u32);
        let (start, inputs, how) = match kept {
            Some(id) => {
                let run = self.recordings.iter().find(|r| r.id == id).ok_or_else(|| format!("there is no run {id}; the person's runs are listed by game_info"))?;
                (run.start, run.replay.clone(), format!("the person's run {id}"))
            }
            None => (start_of(&world, room, at, &abilities)?, inputs_of(input.get("inputs"))?, "the inputs given".into()),
        };
        let from = world.rooms.get(start.0 as usize).map(|r| r.id.clone()).unwrap_or_default();
        let had: Vec<String> = world.abilities.iter().enumerate().filter(|(i, _)| start.2 & (1 << i) != 0).map(|(_, a)| a.clone()).collect();
        let saved = (!project.unsaved().is_empty()).then(|| Project::open(project.root()));
        let played = play_out(world, start, &inputs);

        let mut out = format!(
            "Played {how} from {from} at ({}, {}) with {} — the game as it is now{}.\n{}\n",
            px(start.1[0]),
            px(start.1[1]),
            if had.is_empty() { "no abilities".into() } else { had.join(", ") },
            if saved.is_some() { ", unsaved changes included" } else { "" },
            summary(&played, tick),
        );
        out.push_str("Where it was, every few ticks (tick: room (x, y), g on the ground):\n");
        let places: Vec<String> = played.samples.iter().map(|(t, r, p, g)| format!("{t}: {r} ({}, {}){}", p[0], p[1], if *g { " g" } else { "" })).collect();
        out.push_str(&places.join("; "));
        out.push('\n');

        // The same inputs on the game as last saved: what the changes not yet
        // saved did to the run.
        match saved {
            Some(Ok(before)) => match World::cook(&before) {
                Ok(world) => {
                    let ids: Vec<String> = world.rooms.iter().map(|r| r.id.clone()).collect();
                    match ids.iter().position(|id| *id == from) {
                        Some(index) => {
                            let other = play_out(world, (index as u16, start.1, start.2), &inputs);
                            let same = other.hash == played.hash;
                            let _ = write!(
                                out,
                                "On the game as last saved, the same inputs {}: {}\n",
                                if same { "make exactly the same run" } else { "make a different run" },
                                summary(&other, tick)
                            );
                        }
                        None => {
                            let _ = writeln!(out, "The game as last saved has no room {from}, so the run was not compared.");
                        }
                    }
                }
                Err(why) => {
                    let _ = writeln!(out, "The game as last saved cannot be played ({why}), so the run was not compared.");
                }
            },
            Some(Err(why)) => {
                let _ = writeln!(out, "The game as last saved could not be read ({why}), so the run was not compared.");
            }
            None => {}
        }
        out.push_str("The run is drawn over the room on the person's screen.");
        self.pending.push(Event::GamePath { parts: played.parts });
        Ok(cut(&out, BUDGET).to_string())
    }
}
