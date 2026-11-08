//! C-style exports consumed by `src/sim/worker.ts`. Kept free of wasm-bindgen so
//! the build only needs cargo and the wasm32 target.

pub mod hat;
pub mod hat_world;
pub mod life;
pub mod status;

use hat_world::HatWorld;
use life::{Topology, World};
use status::Status;

/// # Safety
/// All functions below expect a pointer obtained from `world_new`.
#[unsafe(no_mangle)]
pub extern "C" fn world_new() -> *mut World {
    Box::into_raw(Box::new(World::new()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_set_rule(w: *mut World, birth: u32, survive: u32) -> u32 {
    unsafe { (*w).set_rule(birth as u16, survive as u16) as u32 }
}

/// 0 Moore, 1 von Neumann, 2 hexagonal, 3 triangle edges, 4 triangle vertices.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_set_topology(w: *mut World, topology: u32) {
    if let Some(topology) = Topology::from_index(topology) {
        unsafe { (*w).set_topology(topology) }
    }
}

/// Makes this world a torus, or resizes it. Returns 0 if the size is not allowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_set_torus(w: *mut World, width: u32, height: u32) -> u32 {
    let side = |n: u32| n.min(i32::MAX as u32) as i32;
    unsafe { (*w).set_torus(side(width), side(height)) as u32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_step(w: *mut World, n: u32) {
    let w = unsafe { &mut *w };
    for _ in 0..n {
        w.step();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_clear(w: *mut World) {
    unsafe { (*w).clear() }
}

/// Returns a buffer for `n` i32 values, valid until the next call into the engine.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_scratch(w: *mut World, n: u32) -> *mut i32 {
    let w = unsafe { &mut *w };
    w.scratch.resize(n as usize, 0);
    w.scratch.as_mut_ptr()
}

/// Applies `n` (x, y) pairs previously written to the scratch buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_paint(w: *mut World, n: u32, alive: u32) {
    let w = unsafe { &mut *w };
    let cells = std::mem::take(&mut w.scratch);
    for pair in cells[..(n as usize * 2).min(cells.len())].chunks_exact(2) {
        w.set(pair[0], pair[1], alive != 0);
    }
    w.scratch = cells;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_soup(
    w: *mut World,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    density: f64,
    seed: u32,
) {
    unsafe { (*w).soup(x, y, width, height, density, seed) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_frame(
    w: *mut World,
    ox: i32,
    oy: i32,
    width: u32,
    height: u32,
    level: u32,
) -> *const u8 {
    unsafe { (*w).frame(ox, oy, width as usize, height as usize, level) }.as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_generation(w: *const World) -> f64 {
    unsafe { (*w).generation as f64 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_population(w: *const World) -> f64 {
    unsafe { (*w).population as f64 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_active_chunks(w: *const World) -> u32 {
    unsafe { (*w).active_chunks }
}

/// 0 active, 1 empty, 2 still, 3 oscillating, 4 drifting, 5 growing.
fn status_kind(status: Status) -> u32 {
    match status {
        Status::Active => 0,
        Status::Empty => 1,
        Status::Still => 2,
        Status::Oscillating { .. } => 3,
        Status::Drifting { .. } => 4,
        Status::Growing { .. } => 5,
    }
}

fn status_period(status: Status) -> u32 {
    match status {
        Status::Oscillating { period }
        | Status::Drifting { period, .. }
        | Status::Growing { period, .. } => period,
        _ => 0,
    }
}

fn status_detail(status: Status) -> u32 {
    match status {
        Status::Drifting { moving, .. } => moving,
        Status::Growing { delta, .. } => delta,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_status_kind(w: *const World) -> u32 {
    status_kind(unsafe { (*w).detector.status })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_status_period(w: *const World) -> u32 {
    status_period(unsafe { (*w).detector.status })
}

/// Moving objects when drifting, population gain per period when growing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_status_detail(w: *const World) -> u32 {
    status_detail(unsafe { (*w).detector.status })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_status_since(w: *const World) -> f64 {
    unsafe { (*w).detector.since as f64 }
}

/// All live cells as x, y pairs, twice the population in i32 values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_export(w: *mut World) -> *const i32 {
    unsafe { (*w).export_cells() }.as_ptr()
}

/// Writes x0, y0, x1, y1 to the scratch buffer and returns it, or null when empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn world_bounds(w: *mut World) -> *const i32 {
    let w = unsafe { &mut *w };
    match w.bounds() {
        Some((x0, y0, x1, y1)) => {
            w.scratch.clear();
            w.scratch.extend([x0, y0, x1, y1]);
            w.scratch.as_ptr()
        }
        None => std::ptr::null(),
    }
}

// The hat tiling, which lives on its own graph rather than on the integer grid.

#[unsafe(no_mangle)]
pub extern "C" fn hat_new() -> *mut HatWorld {
    Box::into_raw(Box::new(HatWorld::new()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_free(w: *mut HatWorld) {
    drop(unsafe { Box::from_raw(w) });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_set_rule(w: *mut HatWorld, birth: u32, survive: u32) -> u32 {
    unsafe { (*w).set_rule(birth as u16, survive as u16) as u32 }
}

/// Reads `n` values from the scratch buffer, see `set_table_from_scratch`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_set_table(w: *mut HatWorld, n: u32) {
    unsafe { (*w).set_table_from_scratch(n as usize) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_seed_pair(w: *mut HatWorld, cell: u32, first: u32, second: u32) {
    unsafe { (*w).seed_pair(cell, first as u8, second as u8) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_set_edges_only(w: *mut HatWorld, edges_only: u32) {
    unsafe { (*w).set_edges_only(edges_only != 0) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_step(w: *mut HatWorld, n: u32) {
    let w = unsafe { &mut *w };
    for _ in 0..n {
        w.step();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_clear(w: *mut HatWorld) {
    unsafe { (*w).clear() }
}

/// Returns a buffer for `n` u32 values, valid until the next call into the engine.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_scratch(w: *mut HatWorld, n: u32) -> *mut u32 {
    let w = unsafe { &mut *w };
    w.scratch.resize(n as usize, 0);
    w.scratch.as_mut_ptr()
}

/// Applies `n` cell ids previously written to the scratch buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_paint(w: *mut HatWorld, n: u32, alive: u32) {
    unsafe { (*w).paint_scratch(n as usize, alive != 0) }
}

/// Positions are lattice coordinates relative to the hat at the origin.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_soup(w: *mut HatWorld, x: f64, y: f64, radius: f64, density: f64, seed: u32) {
    unsafe { (*w).soup(x, y, radius, density, seed) }
}

/// Generates whatever is missing around a position, and returns the cell count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_load(w: *mut HatWorld, x: f64, y: f64, radius: f64) -> u32 {
    unsafe { (*w).load_around(x, y, radius) };
    unsafe { (*w).cell_count() as u32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_cell_count(w: *const HatWorld) -> u32 {
    unsafe { (*w).cell_count() as u32 }
}

/// Five i32 per cell from `from` on: rotation + 8 * flip, then p.q, p.r, q.q, q.r.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_cells(w: *mut HatWorld, from: u32) -> *const i32 {
    unsafe { (*w).export_cells(from as usize) }.as_ptr()
}

/// Four i32 for each of the outline's 14 vertices: p.q, p.r, q.q, q.r.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_outline(w: *mut HatWorld) -> *const i32 {
    unsafe { (*w).export_outline() }.as_ptr()
}

/// One byte per cell, 255 for live cells.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_states(w: *const HatWorld) -> *const u8 {
    unsafe { (*w).states() }.as_ptr()
}

/// The ratio of b-part to a-part of long distances, as a complex number: `q = k * p`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_ratio_re(w: *const HatWorld) -> f64 {
    unsafe { (*w).ratio() }.0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_ratio_im(w: *const HatWorld) -> f64 {
    unsafe { (*w).ratio() }.1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_capped(w: *const HatWorld) -> u32 {
    unsafe { (*w).capped as u32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_generation(w: *const HatWorld) -> f64 {
    unsafe { (*w).generation as f64 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_population(w: *const HatWorld) -> f64 {
    unsafe { (*w).population as f64 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_status_kind(w: *const HatWorld) -> u32 {
    status_kind(unsafe { (*w).detector.status })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_status_period(w: *const HatWorld) -> u32 {
    status_period(unsafe { (*w).detector.status })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_status_detail(w: *const HatWorld) -> u32 {
    status_detail(unsafe { (*w).detector.status })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hat_status_since(w: *const HatWorld) -> f64 {
    unsafe { (*w).detector.since as f64 }
}
