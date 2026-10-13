//! Game mode's simulation as WebAssembly, for the window and for exported
//! games: `proton-game`'s run behind a plain C ABI, so no binding generator
//! sits between the window and the numbers.
//!
//! The window loads a world the core cooked — JSON, the exact integers the
//! core's own runs use — starts a run, and then steps it once a tick with
//! the input held, reading back a frame of what to draw and what happened.
//! Every function is `pg_`; every pointer is into this module's memory and
//! good until the next call that changes it.
//!
//! ```text
//! pg_abi() → 1
//! pg_alloc(len) → ptr               room for the window to write into
//! pg_free(ptr, len)
//! pg_load(ptr, len) → 0 | -1        a cooked world, as JSON; -1: pg_error()
//! pg_start(room, x, y, abilities)   a run from there, positions in 1/256 px
//! pg_step(input) → happened         one tick; bits of what happened
//! pg_frame() → ptr                  header and drawn records, as i32s
//! pg_hash_lo(), pg_hash_hi()        the run's state, as one number
//! pg_error() → ptr                  why the last call failed: length, then bytes
//! ```

use std::cell::RefCell;

use proton_game::sim::{Happened, Sim, World};

/// The version of this ABI: what the window checks before using it.
pub const ABI: u32 = 1;

/// Words in a frame's header, before its records.
pub const HEADER: usize = 12;
/// Words in a record.
pub const RECORD: usize = 6;

/// What `pg_step` says happened, a bit each.
pub mod happened {
    pub const JUMPED: u32 = 1;
    pub const LANDED: u32 = 2;
    pub const DASHED: u32 = 4;
    pub const HURT: u32 = 8;
    pub const ENTERED: u32 = 16;
    pub const GAINED: u32 = 32;
}

#[derive(Default)]
struct Held {
    world: Option<World>,
    sim: Option<Sim>,
    frame: Vec<i32>,
    error: Vec<u8>,
}

thread_local! {
    static HELD: RefCell<Held> = RefCell::new(Held::default());
}

fn failed(why: String) -> i32 {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let bytes = why.into_bytes();
        h.error = (bytes.len() as u32).to_le_bytes().to_vec();
        h.error.extend(bytes);
    });
    -1
}

#[no_mangle]
pub extern "C" fn pg_abi() -> u32 {
    ABI
}

/// Room for `len` bytes, for the window to write into.
#[no_mangle]
pub extern "C" fn pg_alloc(len: usize) -> *mut u8 {
    let mut room = Vec::<u8>::with_capacity(len.max(1));
    let ptr = room.as_mut_ptr();
    std::mem::forget(room);
    ptr
}

/// Give back what `pg_alloc` gave.
///
/// # Safety
/// `ptr` and `len` must be what `pg_alloc` returned and was asked for.
#[no_mangle]
pub unsafe extern "C" fn pg_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
    }
}

/// Load a cooked world, as JSON.
///
/// # Safety
/// `ptr` must point at `len` bytes this module allocated.
#[no_mangle]
pub unsafe extern "C" fn pg_load(ptr: *const u8, len: usize) -> i32 {
    let bytes = std::slice::from_raw_parts(ptr, len);
    load(bytes)
}

/// Load a cooked world from JSON bytes — the safe half of `pg_load`.
pub fn load(bytes: &[u8]) -> i32 {
    match serde_json::from_slice::<World>(bytes) {
        Ok(world) => {
            if world.rooms.is_empty() {
                return failed("the world has no rooms".into());
            }
            HELD.with(|h| {
                let mut h = h.borrow_mut();
                h.world = Some(world);
                h.sim = None;
            });
            0
        }
        Err(e) => failed(format!("not a world: {e}")),
    }
}

/// Start a run in room `room` at `x`, `y` (1/256 px), with `abilities` had.
#[no_mangle]
pub extern "C" fn pg_start(room: u32, x: i32, y: i32, abilities: u32) -> i32 {
    // Taken out first: saying why it failed borrows the state again.
    let Some(world) = HELD.with(|h| h.borrow().world.clone()) else { return failed("no world is loaded".into()) };
    if room as usize >= world.rooms.len() {
        return failed(format!("there is no room {room}"));
    }
    HELD.with(|h| h.borrow_mut().sim = Some(Sim::at(world, room as u16, [x, y], abilities)));
    0
}

/// One tick with `input` held. Bits of what happened.
#[no_mangle]
pub extern "C" fn pg_step(input: u32) -> u32 {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let Some(sim) = h.sim.as_mut() else { return 0 };
        sim.step(input);
        sim.happened().iter().fold(0, |bits, what| {
            bits | match what {
                Happened::Jumped => happened::JUMPED,
                Happened::Landed => happened::LANDED,
                Happened::Dashed => happened::DASHED,
                Happened::Hurt => happened::HURT,
                Happened::Entered(_) => happened::ENTERED,
                Happened::Gained(_) => happened::GAINED,
            }
        })
    })
}

/// The frame as i32s: a header, then a record a drawn thing.
///
/// Header: abi, tick, room, camera x, camera y (1/256 px), how far through
/// a door (0 none, else 1–24), deaths, abilities, count, player x, player y
/// (1/256 px), grounded. Record: sprite, x, y (pixels), pivot x, pivot y,
/// flipped.
pub fn frame() -> Vec<i32> {
    HELD.with(|h| {
        let h = h.borrow();
        let Some(sim) = h.sim.as_ref() else { return vec![ABI as i32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] };
        let s = &sim.state;
        let drawn = sim.frame();
        let mut out = vec![
            ABI as i32,
            s.tick as i32,
            s.room as i32,
            s.cam[0],
            s.cam[1],
            s.door.map(|(_, _, t)| t as i32 + 1).unwrap_or(0),
            s.deaths as i32,
            s.abilities as i32,
            drawn.len() as i32,
            s.pos[0],
            s.pos[1],
            s.grounded as i32,
        ];
        for d in drawn {
            out.extend([d.sprite as i32, d.at[0], d.at[1], d.pivot[0], d.pivot[1], d.flip as i32]);
        }
        out
    })
}

#[no_mangle]
pub extern "C" fn pg_frame() -> *const i32 {
    let words = frame();
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        h.frame = words;
        h.frame.as_ptr()
    })
}

/// The run's state hash: two runs with the same one are the same run.
pub fn hash() -> u64 {
    HELD.with(|h| h.borrow().sim.as_ref().map(Sim::hash).unwrap_or(0))
}

#[no_mangle]
pub extern "C" fn pg_hash_lo() -> u32 {
    hash() as u32
}

#[no_mangle]
pub extern "C" fn pg_hash_hi() -> u32 {
    (hash() >> 32) as u32
}

#[no_mangle]
pub extern "C" fn pg_error() -> *const u8 {
    HELD.with(|h| h.borrow().error.as_ptr())
}
