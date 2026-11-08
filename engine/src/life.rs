//! Unbounded two-state automaton on an integer grid.
//!
//! The field is a hash map of 64x64 chunks, one `u64` per row. A step counts
//! neighbours for 64 cells at once with bit-sliced adders, and skips chunks whose
//! surroundings did not change in the previous generation.
//!
//! All periodic tilings share this grid and differ only in their neighbourhood;
//! how grid cells map to polygons on screen is up to the renderer.
//!
//! A world can also be a torus of fixed size, see `Torus`.

use crate::status::Detector;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

const BITS: i32 = 6;
pub(crate) const SIZE: usize = 1 << BITS;
const NONE: u32 = u32::MAX;
const EMPTY: [u64; SIZE] = [0; SIZE];
const GC_INTERVAL: u64 = 64;
const MIN_TORUS: i32 = 8;
const MAX_TORUS: i32 = 8192;

/// Link order: N, S, W, E, NW, NE, SW, SE.
const OFFSETS: [(i32, i32); 8] = [
    (0, -1),
    (0, 1),
    (-1, 0),
    (1, 0),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

/// Cell offsets as (dx, dy). Cells with even x + y use `even`, the others `odd`,
/// which only differ on the triangular grid.
pub struct Neighbourhood {
    even: &'static [(i32, i32)],
    odd: &'static [(i32, i32)],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Topology {
    Moore,
    VonNeumann,
    Hexagonal,
    TriangleEdges,
    TriangleVertices,
}

const VON_NEUMANN: &[(i32, i32)] = &[(-1, 0), (1, 0), (0, -1), (0, 1)];
const HEXAGONAL: &[(i32, i32)] = &[(-1, 0), (1, 0), (0, -1), (0, 1), (1, -1), (-1, 1)];
const TRIANGLE_EDGES_EVEN: &[(i32, i32)] = &[(-1, 0), (1, 0), (0, -1)];
const TRIANGLE_EDGES_ODD: &[(i32, i32)] = &[(-1, 0), (1, 0), (0, 1)];
const TRIANGLE_VERTICES_EVEN: &[(i32, i32)] = &[
    (-2, 0), (-1, 0), (1, 0), (2, 0),
    (-2, -1), (-1, -1), (0, -1), (1, -1), (2, -1),
    (-1, 1), (0, 1), (1, 1),
];
const TRIANGLE_VERTICES_ODD: &[(i32, i32)] = &[
    (-2, 0), (-1, 0), (1, 0), (2, 0),
    (-2, 1), (-1, 1), (0, 1), (1, 1), (2, 1),
    (-1, -1), (0, -1), (1, -1),
];
const MOORE: &[(i32, i32)] = &[
    (-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1),
];

impl Topology {
    pub fn from_index(index: u32) -> Option<Self> {
        Some(match index {
            0 => Topology::Moore,
            1 => Topology::VonNeumann,
            2 => Topology::Hexagonal,
            3 => Topology::TriangleEdges,
            4 => Topology::TriangleVertices,
            _ => return None,
        })
    }

    pub fn neighbourhood(self) -> Neighbourhood {
        let (even, odd) = match self {
            Topology::Moore => (MOORE, MOORE),
            Topology::VonNeumann => (VON_NEUMANN, VON_NEUMANN),
            Topology::Hexagonal => (HEXAGONAL, HEXAGONAL),
            Topology::TriangleEdges => (TRIANGLE_EDGES_EVEN, TRIANGLE_EDGES_ODD),
            Topology::TriangleVertices => (TRIANGLE_VERTICES_EVEN, TRIANGLE_VERTICES_ODD),
        };
        Neighbourhood { even, odd }
    }

    /// Largest distance on either axis at which a cell has a neighbour.
    pub fn range(self) -> i32 {
        if self == Topology::TriangleVertices { 2 } else { 1 }
    }

    /// On the triangular grid, an odd translation turns a triangle upside down,
    /// so a shape shifted that way is not the same object.
    pub fn parity_matters(self) -> bool {
        matches!(self, Topology::TriangleEdges | Topology::TriangleVertices)
    }
}

#[derive(Default)]
pub struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u32(b as u32);
        }
    }
    fn write_u64(&mut self, v: u64) {
        self.write_u32(v as u32);
        self.write_u32((v >> 32) as u32);
    }
    fn write_i64(&mut self, v: i64) {
        self.write_u32(v as u32);
        self.write_u32((v >> 32) as u32);
    }
    fn write_i32(&mut self, v: i32) {
        self.write_u32(v as u32);
    }
    fn write_u32(&mut self, v: u32) {
        self.0 = (self.0.rotate_left(26) ^ v as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type Index = HashMap<(i32, i32), u32, BuildHasherDefault<KeyHasher>>;

pub(crate) struct Chunk {
    pub(crate) key: (i32, i32),
    buf: [[u64; SIZE]; 2],
    front: usize,
    links: [u32; 8],
    pub(crate) pop: u32,
    next_pop: u32,
    /// Differs from the previous generation, so neighbours must be recomputed.
    pub(crate) changed: bool,
    next_changed: bool,
    active: bool,
    /// Index into `Torus::masks` for a chunk that reaches past the torus' edge.
    mask: u32,
}

impl Chunk {
    fn new(key: (i32, i32)) -> Self {
        Chunk {
            key,
            buf: [EMPTY; 2],
            front: 0,
            links: [NONE; 8],
            pop: 0,
            next_pop: 0,
            changed: false,
            next_changed: false,
            active: false,
            mask: NONE,
        }
    }

    pub(crate) fn rows(&self) -> &[u64; SIZE] {
        &self.buf[self.front]
    }

    /// Links in `OFFSETS` order that a live border cell could spill into.
    fn spill(&self) -> u8 {
        let rows = self.rows();
        let (mut west, mut east) = (0u64, 0u64);
        for r in rows {
            // Two columns, because triangle vertices reach two cells sideways.
            west |= r & 3;
            east |= r >> 62;
        }
        let (top, bottom) = (rows[0], rows[SIZE - 1]);
        (top != 0) as u8
            | ((bottom != 0) as u8) << 1
            | ((west != 0) as u8) << 2
            | ((east != 0) as u8) << 3
            | (((top & 3) != 0) as u8) << 4
            | (((top >> 62) != 0) as u8) << 5
            | (((bottom & 3) != 0) as u8) << 6
            | (((bottom >> 62) != 0) as u8) << 7
    }
}

/// A field of fixed size whose opposite edges touch.
///
/// The kernels stay as they are: before each step, the cells just past the edges
/// are filled with copies from the opposite side, and the kernels read them like
/// any other neighbour. Those ghosts lie in chunks of their own and in the unused
/// part of chunks at the far edges, which `masks` wipes again after the step.
///
/// Reach is two columns and one row, the most any neighbourhood needs. Sizes are
/// even, which any tile family but the hexagon needs for its edges to fit. It also
/// means that two adjacent ghost columns share a word, and that the last chunk
/// column holds at least two cells, so influence never skips a chunk.
///
/// All chunks exist from the start and never move, so a position gives the index.
pub(crate) struct Torus {
    pub(crate) width: i32,
    pub(crate) height: i32,
    /// Chunks holding cells of the field, per axis.
    cols: i32,
    rows: i32,
    /// Chunks per row of the whole layout, ghosts included.
    stride: i32,
    masks: Vec<[u64; SIZE]>,
}

impl Torus {
    /// `cx` runs from -1 to `width >> 6`, `cy` from -1 to `height >> 6`.
    #[inline(always)]
    fn id(&self, cx: i32, cy: i32) -> usize {
        ((cy + 1) * self.stride + cx + 1) as usize
    }
}

pub struct World {
    pub(crate) chunks: Vec<Chunk>,
    index: Index,
    pub(crate) topology: Topology,
    pub(crate) torus: Option<Torus>,
    pub(crate) birth: u16,
    survive: u16,
    pub generation: u64,
    pub population: u64,
    pub active_chunks: u32,
    pub detector: Detector,
    pub scratch: Vec<i32>,
    frame: Vec<u8>,
    spawn: Vec<(i32, i32)>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[inline(always)]
fn count_equals(k: u32, c0: u64, c1: u64, c2: u64, c3: u64) -> u64 {
    let pick = |bit: u32, v: u64| if k >> bit & 1 == 1 { v } else { !v };
    pick(0, c0) & pick(1, c1) & pick(2, c2) & pick(3, c3)
}

impl World {
    pub fn new() -> Self {
        World {
            chunks: Vec::new(),
            index: Index::default(),
            topology: Topology::Moore,
            torus: None,
            birth: 1 << 3,
            survive: 1 << 2 | 1 << 3,
            generation: 0,
            population: 0,
            active_chunks: 0,
            detector: Detector::default(),
            scratch: Vec::new(),
            frame: Vec::new(),
            spawn: Vec::new(),
        }
    }

    /// Turns this world into a torus of the given size, or resizes it. Cells inside
    /// the new bounds stay, and the generation count starts over. Returns false and
    /// changes nothing if the size is odd or out of range.
    pub fn set_torus(&mut self, width: i32, height: i32) -> bool {
        let valid = |n: i32| n % 2 == 0 && (MIN_TORUS..=MAX_TORUS).contains(&n);
        if !valid(width) || !valid(height) {
            return false;
        }
        let mut live = Vec::new();
        for c in self.chunks.iter().filter(|c| c.pop > 0) {
            for (y, &row) in c.rows().iter().enumerate() {
                let mut bits = row;
                while bits != 0 {
                    let x = c.key.0 * SIZE as i32 + bits.trailing_zeros() as i32;
                    let y = c.key.1 * SIZE as i32 + y as i32;
                    bits &= bits - 1;
                    if (0..width).contains(&x) && (0..height).contains(&y) {
                        live.push((x, y));
                    }
                }
            }
        }

        self.chunks.clear();
        self.index.clear();
        let mut torus = Torus {
            width,
            height,
            cols: (width + SIZE as i32 - 1) >> BITS,
            rows: (height + SIZE as i32 - 1) >> BITS,
            stride: (width >> BITS) + 2,
            masks: Vec::new(),
        };
        for cy in -1..=height >> BITS {
            for cx in -1..=width >> BITS {
                let i = self.chunk_at((cx, cy));
                debug_assert_eq!(i, torus.id(cx, cy));
                if cx < 0 || cy < 0 || cx >= torus.cols || cy >= torus.rows {
                    continue;
                }
                let mut mask = EMPTY;
                let mut partial = false;
                for (ry, row) in mask.iter_mut().enumerate() {
                    let columns = (width - cx * SIZE as i32).min(SIZE as i32) as u32;
                    let inside = cy * SIZE as i32 + (ry as i32) < height;
                    *row = match (inside, columns) {
                        (false, _) => 0,
                        (true, 64) => !0,
                        (true, n) => (1u64 << n) - 1,
                    };
                    partial |= *row != !0;
                }
                if partial {
                    self.chunks[i].mask = torus.masks.len() as u32;
                    torus.masks.push(mask);
                }
            }
        }
        self.torus = Some(torus);
        self.clear();
        for (x, y) in live {
            self.set(x, y, true);
        }
        true
    }

    /// Masks hold one bit per neighbour count. B0 would fill the unbounded plane
    /// in a single step, so it is rejected there and the previous rule stays in
    /// place. A torus is finite and can take it.
    pub fn set_rule(&mut self, birth: u16, survive: u16) -> bool {
        if birth & 1 != 0 && self.torus.is_none() {
            return false;
        }
        self.birth = birth;
        self.survive = survive;
        for c in &mut self.chunks {
            c.changed = true;
        }
        self.detector.invalidate();
        true
    }

    /// Keeps the live cells, which then mean whatever the new grid makes of them.
    pub fn set_topology(&mut self, topology: Topology) {
        self.topology = topology;
        for c in &mut self.chunks {
            c.changed = true;
        }
        self.detector.invalidate();
    }

    pub fn clear(&mut self) {
        if self.torus.is_some() {
            // Under B0 an empty field is not at rest, so everything has to be looked at.
            for c in &mut self.chunks {
                c.buf = [EMPTY; 2];
                c.pop = 0;
                c.changed = true;
            }
        } else {
            self.chunks.clear();
            self.index.clear();
        }
        self.generation = 0;
        self.population = 0;
        self.active_chunks = 0;
        self.detector.invalidate();
    }

    fn chunk_at(&mut self, key: (i32, i32)) -> usize {
        if let Some(&i) = self.index.get(&key) {
            return i as usize;
        }
        let i = self.chunks.len() as u32;
        let mut chunk = Chunk::new(key);
        for (l, (dx, dy)) in OFFSETS.iter().enumerate() {
            if let Some(&n) = self.index.get(&(key.0 + dx, key.1 + dy)) {
                chunk.links[l] = n;
                // The opposite direction sits next to `l` in OFFSETS.
                self.chunks[n as usize].links[opposite(l)] = i;
            }
        }
        self.chunks.push(chunk);
        self.index.insert(key, i);
        i as usize
    }

    /// On a torus, positions wrap around.
    pub fn set(&mut self, x: i32, y: i32, alive: bool) {
        let (x, y) = match &self.torus {
            Some(t) => (x.rem_euclid(t.width), y.rem_euclid(t.height)),
            None => (x, y),
        };
        let key = (x >> BITS, y >> BITS);
        let i = if alive {
            self.chunk_at(key)
        } else {
            match self.index.get(&key) {
                Some(&i) => i as usize,
                None => return,
            }
        };
        let c = &mut self.chunks[i];
        let row = &mut c.buf[c.front][(y & (SIZE as i32 - 1)) as usize];
        let bit = 1u64 << (x & (SIZE as i32 - 1));
        if (*row & bit != 0) != alive {
            *row ^= bit;
            c.changed = true;
            self.detector.invalidate();
            if alive {
                c.pop += 1;
                self.population += 1;
            } else {
                c.pop -= 1;
                self.population -= 1;
            }
        }
    }

    pub fn soup(&mut self, x: i32, y: i32, width: i32, height: i32, density: f64, seed: u32) {
        let threshold = (density.clamp(0.0, 1.0) * u32::MAX as f64) as u32;
        let mut s = seed | 1;
        for cy in y..y.saturating_add(height) {
            for cx in x..x.saturating_add(width) {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                self.set(cx, cy, s < threshold);
            }
        }
    }

    pub fn step(&mut self) {
        if self.torus.is_some() {
            self.fill_ghosts();
            self.mark_active_on_torus();
        } else {
            self.expand();
            self.mark_active();
        }

        let (birth, survive) = (self.birth, self.survive);
        let topology = self.topology;
        let neighbourhood = topology.neighbourhood();
        let mut active = 0;
        for i in 0..self.chunks.len() {
            if !self.chunks[i].active {
                continue;
            }
            active += 1;
            let c = &self.chunks[i];
            let linked = |l: usize| -> &[u64; SIZE] {
                match c.links[l] {
                    NONE => &EMPTY,
                    n => self.chunks[n as usize].rows(),
                }
            };
            let around = [
                linked(0),
                linked(1),
                linked(2),
                linked(3),
                linked(4),
                linked(5),
                linked(6),
                linked(7),
            ];
            let mut next = EMPTY;
            let (changed, pop) = if topology == Topology::Moore {
                advance(c.rows(), &around, birth, survive, &mut next)
            } else {
                advance_general(c.rows(), &around, &neighbourhood, birth, survive, &mut next)
            };
            // Ghosts must neither survive into the next generation nor count as change.
            let (changed, pop) = match (c.mask, &self.torus) {
                (NONE, _) | (_, None) => (changed, pop),
                (m, Some(t)) => {
                    let (mask, me) = (&t.masks[m as usize], c.rows());
                    let (mut changed, mut pop) = (false, 0);
                    for y in 0..SIZE {
                        next[y] &= mask[y];
                        changed |= next[y] != me[y] & mask[y];
                        pop += next[y].count_ones();
                    }
                    (changed, pop)
                }
            };
            let c = &mut self.chunks[i];
            let back = 1 - c.front;
            c.buf[back] = next;
            c.next_changed = changed;
            c.next_pop = pop;
        }
        self.active_chunks = active;

        let mut population = 0u64;
        for c in &mut self.chunks {
            if c.active {
                c.front = 1 - c.front;
                c.pop = c.next_pop;
                c.changed = c.next_changed;
            } else {
                c.changed = false;
                if let (true, Some(t)) = (c.mask != NONE, &self.torus) {
                    let front = c.front;
                    let mask = &t.masks[c.mask as usize];
                    c.buf[front].iter_mut().zip(mask).for_each(|(row, m)| *row &= m);
                }
            }
            population += c.pop as u64;
        }
        self.population = population;
        self.generation += 1;

        if self.generation % GC_INTERVAL == 0 && self.torus.is_none() {
            self.collect();
        }
        self.observe();
    }

    /// Writes all live cells to the scratch buffer as x, y pairs.
    pub fn export_cells(&mut self) -> &[i32] {
        self.scratch.clear();
        for c in self.chunks.iter().filter(|c| c.pop > 0) {
            for (y, &row) in c.rows().iter().enumerate() {
                let wy = c.key.1 * SIZE as i32 + y as i32;
                let mut bits = row;
                while bits != 0 {
                    self.scratch.push(c.key.0 * SIZE as i32 + bits.trailing_zeros() as i32);
                    self.scratch.push(wy);
                    bits &= bits - 1;
                }
            }
        }
        &self.scratch
    }

    /// Bounding box of all live cells as (x0, y0, x1, y1), if there are any.
    pub fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
        let mut bounds = None;
        for c in self.chunks.iter().filter(|c| c.pop > 0) {
            for (y, &row) in c.rows().iter().enumerate() {
                if row == 0 {
                    continue;
                }
                let wy = c.key.1 * SIZE as i32 + y as i32;
                let left = c.key.0 * SIZE as i32 + row.trailing_zeros() as i32;
                let right = c.key.0 * SIZE as i32 + 63 - row.leading_zeros() as i32;
                let (x0, y0, x1, y1) = bounds.unwrap_or((left, wy, right, wy));
                bounds = Some((x0.min(left), y0.min(wy), x1.max(right), y1.max(wy)));
            }
        }
        bounds
    }

    /// Creates missing chunks next to changed chunks with live border cells.
    fn expand(&mut self) {
        let mut spawn = std::mem::take(&mut self.spawn);
        spawn.clear();
        for c in &self.chunks {
            if !c.changed {
                continue;
            }
            let spill = c.spill();
            for (l, (dx, dy)) in OFFSETS.iter().enumerate() {
                if c.links[l] == NONE && spill >> l & 1 == 1 {
                    spawn.push((c.key.0 + dx, c.key.1 + dy));
                }
            }
        }
        for &key in &spawn {
            self.chunk_at(key);
        }
        self.spawn = spawn;
    }

    fn mark_active(&mut self) {
        for i in 0..self.chunks.len() {
            let c = &self.chunks[i];
            let active = c.changed
                || c.links
                    .iter()
                    .any(|&n| n != NONE && self.chunks[n as usize].changed);
            self.chunks[i].active = active;
        }
    }

    /// Copies the cells along each edge to just past the opposite one. Rows go
    /// second and include the ghost columns, which fills the corners too.
    fn fill_ghosts(&mut self) {
        let Some(t) = &self.torus else { return };
        let chunks = &mut self.chunks;
        let (width, height) = (t.width, t.height);
        let last = width - 2;
        let (far_col, far_bit) = (width >> BITS, width & 63);
        for y in 0..height {
            let (cy, row) = (y >> BITS, (y & 63) as usize);
            let first_two = chunks[t.id(0, cy)].rows()[row] & 3;
            let last_two = chunks[t.id(last >> BITS, cy)].rows()[row] >> (last & 63) & 3;

            let west = &mut chunks[t.id(-1, cy)];
            west.buf[west.front][row] = last_two << 62;
            let east = &mut chunks[t.id(far_col, cy)];
            let word = &mut east.buf[east.front][row];
            *word = *word & !(3 << far_bit) | first_two << far_bit;
        }
        for cx in -1..=far_col {
            let bottom = chunks[t.id(cx, (height - 1) >> BITS)].rows()[((height - 1) & 63) as usize];
            let north = &mut chunks[t.id(cx, -1)];
            north.buf[north.front][SIZE - 1] = bottom;
            let top = chunks[t.id(cx, 0)].rows()[0];
            let south = &mut chunks[t.id(cx, height >> BITS)];
            south.buf[south.front][(height & 63) as usize] = top;
        }
    }

    /// Ghost chunks never run. Links stop at the edges, so this goes by position.
    fn mark_active_on_torus(&mut self) {
        let Some(t) = &self.torus else { return };
        for cy in 0..t.rows {
            for cx in 0..t.cols {
                let mut active = false;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let n = t.id((cx + dx).rem_euclid(t.cols), (cy + dy).rem_euclid(t.rows));
                        active |= self.chunks[n].changed;
                    }
                }
                self.chunks[t.id(cx, cy)].active = active;
            }
        }
    }

    /// Drops empty chunks with no populated neighbour, then rebuilds all links
    /// because removal moves chunks around.
    fn collect(&mut self) {
        let keep: Vec<bool> = self
            .chunks
            .iter()
            .map(|c| {
                c.pop > 0
                    || c.links
                        .iter()
                        .any(|&n| n != NONE && self.chunks[n as usize].pop > 0)
            })
            .collect();
        if keep.iter().all(|&k| k) {
            return;
        }
        let mut flags = keep.iter();
        self.chunks.retain(|_| *flags.next().unwrap());
        self.index.clear();
        for (i, c) in self.chunks.iter().enumerate() {
            self.index.insert(c.key, i as u32);
        }
        for i in 0..self.chunks.len() {
            let key = self.chunks[i].key;
            for (l, (dx, dy)) in OFFSETS.iter().enumerate() {
                let n = self.index.get(&(key.0 + dx, key.1 + dy));
                self.chunks[i].links[l] = n.copied().unwrap_or(NONE);
            }
        }
    }

    /// Rasterises a window into one byte per texel, each covering `2^level`
    /// cells per axis. Bytes hold coverage, 255 being a fully live texel.
    ///
    /// On a torus, the window starts at `ox`, `oy` and wraps around the edges. It
    /// must not be larger than the field, the renderer repeats it from there on.
    pub fn frame(&mut self, ox: i32, oy: i32, width: usize, height: usize, level: u32) -> &[u8] {
        self.frame.clear();
        self.frame.resize(width * height, 0);
        let (ox, oy) = (ox as i64, oy as i64);
        let (right, bottom) = (
            ox + ((width as i64) << level),
            oy + ((height as i64) << level),
        );
        let size = SIZE as i64;
        let increment = (255u32 >> (2 * level).min(8)).max(1) as u8;

        if let Some(t) = &self.torus {
            let (span_x, span_y) = ((width as i64) << level, (height as i64) << level);
            for c in self.chunks.iter().filter(|c| c.pop > 0) {
                let (cx, cy) = ((c.key.0 as i64) * size, (c.key.1 as i64) * size);
                for (y, &row) in c.rows().iter().enumerate() {
                    let ry = (cy + y as i64 - oy).rem_euclid(t.height as i64);
                    if row == 0 || ry >= span_y {
                        continue;
                    }
                    let ty = (ry >> level) as usize;
                    let mut bits = row;
                    while bits != 0 {
                        let rx = (cx + bits.trailing_zeros() as i64 - ox).rem_euclid(t.width as i64);
                        bits &= bits - 1;
                        if rx < span_x {
                            let texel = &mut self.frame[ty * width + (rx >> level) as usize];
                            *texel = texel.saturating_add(increment);
                        }
                    }
                }
            }
            return &self.frame;
        }

        for c in &self.chunks {
            if c.pop == 0 {
                continue;
            }
            let (cx, cy) = ((c.key.0 as i64) * size, (c.key.1 as i64) * size);
            if cx + size <= ox || cx >= right || cy + size <= oy || cy >= bottom {
                continue;
            }
            if level >= BITS as u32 {
                // The chunk falls into exactly one texel, because texels are
                // aligned to their own size.
                let tx = ((cx - ox) >> level) as usize;
                let ty = ((cy - oy) >> level) as usize;
                let texel = &mut self.frame[ty * width + tx];
                *texel = texel.saturating_add(c.pop.min(255) as u8);
                continue;
            }
            for (y, &row) in c.rows().iter().enumerate() {
                let wy = cy + y as i64;
                if row == 0 || wy < oy || wy >= bottom {
                    continue;
                }
                let ty = ((wy - oy) >> level) as usize;
                let mut bits = row;
                while bits != 0 {
                    let wx = cx + bits.trailing_zeros() as i64;
                    bits &= bits - 1;
                    if wx >= ox && wx < right {
                        let tx = ((wx - ox) >> level) as usize;
                        let texel = &mut self.frame[ty * width + tx];
                        *texel = texel.saturating_add(increment);
                    }
                }
            }
        }
        &self.frame
    }
}

/// Index of the link pointing back along link `l`.
fn opposite(l: usize) -> usize {
    match l {
        0 => 1,
        1 => 0,
        2 => 3,
        3 => 2,
        4 => 7,
        5 => 6,
        6 => 5,
        _ => 4,
    }
}

#[inline(always)]
fn full_add(a: u64, b: u64, c: u64) -> (u64, u64) {
    let x = a ^ b;
    (x ^ c, (a & b) | (x & c))
}

/// Computes one chunk. `around` follows the `OFFSETS` order. Bit x of a row is
/// column x, so a cell's western neighbour moves to it by shifting left.
#[inline(always)]
fn advance(
    me: &[u64; SIZE],
    around: &[&[u64; SIZE]; 8],
    birth: u16,
    survive: u16,
    out: &mut [u64; SIZE],
) -> (bool, u32) {
    let [north, south, west, east, nw, ne, sw, se] = *around;
    let mut changed = false;
    let mut pop = 0;

    for y in 0..SIZE {
        let (a, al, ar) = if y == 0 {
            (north[SIZE - 1], nw[SIZE - 1], ne[SIZE - 1])
        } else {
            (me[y - 1], west[y - 1], east[y - 1])
        };
        let (b, bl, br) = (me[y], west[y], east[y]);
        let (c, cl, cr) = if y == SIZE - 1 {
            (south[0], sw[0], se[0])
        } else {
            (me[y + 1], west[y + 1], east[y + 1])
        };

        let n = [
            (a << 1) | (al >> 63),
            a,
            (a >> 1) | (ar << 63),
            (b << 1) | (bl >> 63),
            (b >> 1) | (br << 63),
            (c << 1) | (cl >> 63),
            c,
            (c >> 1) | (cr << 63),
        ];

        // Eight one-bit inputs reduced to a four-bit count per column.
        let (s1, c1) = full_add(n[0], n[1], n[2]);
        let (s2, c2) = full_add(n[3], n[4], n[5]);
        let (s3, c3) = (n[6] ^ n[7], n[6] & n[7]);
        let (bit0, c4) = full_add(s1, s2, s3);
        let (s5, c5) = full_add(c1, c2, c3);
        let (bit1, c6) = (s5 ^ c4, s5 & c4);
        let (bit2, bit3) = (c5 ^ c6, c5 & c6);

        let mut born = 0;
        let mut stay = 0;
        for k in 0..=8 {
            if (birth | survive) >> k & 1 == 1 {
                let equal = count_equals(k, bit0, bit1, bit2, bit3);
                if birth >> k & 1 == 1 {
                    born |= equal;
                }
                if survive >> k & 1 == 1 {
                    stay |= equal;
                }
            }
        }

        let next = (b & stay) | (!b & born);
        out[y] = next;
        changed |= next != b;
        pop += next.count_ones();
    }
    (changed, pop)
}

/// Brings the bits of a neighbour `dx` columns away onto the cell's own column.
#[inline(always)]
fn shifted(word: u64, left: u64, right: u64, dx: i32) -> u64 {
    match dx {
        0 => word,
        d if d < 0 => (word << -d) | (left >> (64 + d)),
        d => (word >> d) | (right << (64 - d)),
    }
}

/// Slower than `advance`, but works for any neighbourhood of up to 15 cells.
fn advance_general(
    me: &[u64; SIZE],
    around: &[&[u64; SIZE]; 8],
    neighbourhood: &Neighbourhood,
    birth: u16,
    survive: u16,
    out: &mut [u64; SIZE],
) -> (bool, u32) {
    let [north, south, west, east, nw, ne, sw, se] = *around;
    let mut changed = false;
    let mut pop = 0;

    for y in 0..SIZE {
        let row = |dy: i32| -> (u64, u64, u64) {
            match y as i32 + dy {
                -1 => (north[SIZE - 1], nw[SIZE - 1], ne[SIZE - 1]),
                r if r == SIZE as i32 => (south[0], sw[0], se[0]),
                r => (me[r as usize], west[r as usize], east[r as usize]),
            }
        };
        // Chunks start on even coordinates, so parity only depends on x and the row.
        let even_cells = if y % 2 == 0 {
            0x5555_5555_5555_5555u64
        } else {
            0xAAAA_AAAA_AAAA_AAAAu64
        };

        let (mut c0, mut c1, mut c2, mut c3) = (0u64, 0u64, 0u64, 0u64);
        let mut add = |offsets: &[(i32, i32)], mask: u64| {
            for &(dx, dy) in offsets {
                let (word, left, right) = row(dy);
                let mut carry = shifted(word, left, right, dx) & mask;
                let next = c0 & carry;
                c0 ^= carry;
                carry = next;
                let next = c1 & carry;
                c1 ^= carry;
                carry = next;
                let next = c2 & carry;
                c2 ^= carry;
                carry = next;
                c3 ^= carry;
            }
        };
        if std::ptr::eq(neighbourhood.even, neighbourhood.odd) {
            add(neighbourhood.even, !0);
        } else {
            add(neighbourhood.even, even_cells);
            add(neighbourhood.odd, !even_cells);
        }

        let mut born = 0;
        let mut stay = 0;
        for k in 0..16 {
            if (birth | survive) >> k & 1 == 1 {
                let equal = count_equals(k, c0, c1, c2, c3);
                if birth >> k & 1 == 1 {
                    born |= equal;
                }
                if survive >> k & 1 == 1 {
                    stay |= equal;
                }
            }
        }

        let b = me[y];
        let next = (b & stay) | (!b & born);
        out[y] = next;
        changed |= next != b;
        pop += next.count_ones();
    }
    (changed, pop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    type Cells = HashSet<(i32, i32)>;

    fn naive_step(cells: &Cells, birth: u16, survive: u16) -> Cells {
        let mut counts: HashMap<(i32, i32), u32> = HashMap::new();
        for &(x, y) in cells {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx != 0 || dy != 0 {
                        *counts.entry((x + dx, y + dy)).or_default() += 1;
                    }
                }
            }
        }
        counts
            .into_iter()
            .filter(|(p, n)| {
                let mask = if cells.contains(p) { survive } else { birth };
                mask >> n & 1 == 1
            })
            .map(|(p, _)| p)
            .collect()
    }

    fn cells_of(w: &World) -> Cells {
        let mut out = Cells::new();
        for c in &w.chunks {
            for (y, &row) in c.rows().iter().enumerate() {
                let mut bits = row;
                while bits != 0 {
                    let x = bits.trailing_zeros() as i32;
                    bits &= bits - 1;
                    out.insert((c.key.0 * 64 + x, c.key.1 * 64 + y as i32));
                }
            }
        }
        out
    }

    fn compare_with_naive(birth: u16, survive: u16, density: f64, seed: u32) {
        let mut w = World::new();
        assert!(w.set_rule(birth, survive));
        // Straddles the chunk boundaries around the origin in all quadrants.
        w.soup(-90, -70, 180, 140, density, seed);
        let mut reference = cells_of(&w);
        for generation in 0..130 {
            w.step();
            reference = naive_step(&reference, birth, survive);
            assert_eq!(cells_of(&w), reference, "diverged at step {generation}");
            assert_eq!(w.population as usize, reference.len());
        }
    }

    fn naive_general(cells: &Cells, topology: Topology, birth: u16, survive: u16) -> Cells {
        let n = topology.neighbourhood();
        let mut counts: HashMap<(i32, i32), u32> = HashMap::new();
        for &(x, y) in cells {
            // Neighbourhoods are symmetric, so a cell's neighbours are also
            // exactly the cells that count it.
            counts.entry((x, y)).or_default();
            let offsets = if (x + y).rem_euclid(2) == 0 { n.even } else { n.odd };
            for &(dx, dy) in offsets {
                *counts.entry((x + dx, y + dy)).or_default() += 1;
            }
        }
        counts
            .into_iter()
            .filter(|(p, c)| {
                let mask = if cells.contains(p) { survive } else { birth };
                mask >> c & 1 == 1
            })
            .map(|(p, _)| p)
            .collect()
    }

    fn compare_topology(topology: Topology, birth: u16, survive: u16, seed: u32) {
        let mut w = World::new();
        w.set_topology(topology);
        assert!(w.set_rule(birth, survive));
        w.soup(-90, -70, 180, 140, 0.3, seed);
        let mut reference = cells_of(&w);
        for generation in 0..100 {
            w.step();
            reference = naive_general(&reference, topology, birth, survive);
            assert_eq!(cells_of(&w), reference, "{topology:?} diverged at step {generation}");
        }
    }

    #[test]
    fn neighbourhoods_are_symmetric() {
        for index in 0..5 {
            let topology = Topology::from_index(index).unwrap();
            let n = topology.neighbourhood();
            for (list, flip) in [(n.even, n.odd), (n.odd, n.even)] {
                for &(dx, dy) in list {
                    let target = if (dx + dy).rem_euclid(2) == 0 { list } else { flip };
                    assert!(target.contains(&(-dx, -dy)), "{topology:?} {dx},{dy}");
                }
            }
        }
    }

    #[test]
    fn general_kernel_agrees_with_the_fast_one_on_moore() {
        let mut fast = World::new();
        fast.soup(-90, -70, 180, 140, 0.3, 99);
        let mut reference = cells_of(&fast);
        for _ in 0..100 {
            fast.step();
            reference = naive_general(&reference, Topology::Moore, 1 << 3, 1 << 2 | 1 << 3);
            assert_eq!(cells_of(&fast), reference);
        }
    }

    #[test]
    fn matches_naive_von_neumann() {
        compare_topology(Topology::VonNeumann, 1 << 1 | 1 << 3, 1 << 0 | 1 << 2, 11);
    }

    #[test]
    fn matches_naive_hexagonal() {
        compare_topology(Topology::Hexagonal, 1 << 2, 1 << 3 | 1 << 4, 22);
    }

    #[test]
    fn matches_naive_triangle_edges() {
        compare_topology(Topology::TriangleEdges, 1 << 1 | 1 << 2, 1 << 1, 33);
    }

    #[test]
    fn matches_naive_triangle_vertices() {
        compare_topology(Topology::TriangleVertices, 1 << 4, 1 << 4 | 1 << 5 | 1 << 6, 44);
    }

    #[test]
    fn matches_naive_conway() {
        compare_with_naive(1 << 3, 1 << 2 | 1 << 3, 0.3, 12345);
    }

    #[test]
    fn matches_naive_highlife() {
        compare_with_naive(1 << 3 | 1 << 6, 1 << 2 | 1 << 3, 0.25, 777);
    }

    #[test]
    fn matches_naive_day_and_night() {
        let birth = 1 << 3 | 1 << 6 | 1 << 7 | 1 << 8;
        let survive = 1 << 3 | 1 << 4 | 1 << 6 | 1 << 7 | 1 << 8;
        compare_with_naive(birth, survive, 0.5, 4242);
    }

    #[test]
    fn glider_travels_across_chunks_and_empty_chunks_are_collected() {
        let mut w = World::new();
        let glider = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
        for (x, y) in glider {
            w.set(x + 60, y + 60, true);
        }
        for _ in 0..4 * 500 {
            w.step();
        }
        let expected: Cells = glider.iter().map(|&(x, y)| (x + 560, y + 560)).collect();
        assert_eq!(cells_of(&w), expected);
        assert!(w.chunks.len() <= 16, "{} chunks left", w.chunks.len());
    }

    #[test]
    fn still_life_goes_idle() {
        let mut w = World::new();
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            w.set(x, y, true);
        }
        for _ in 0..5 {
            w.step();
        }
        assert_eq!(w.active_chunks, 0);
        assert_eq!(w.population, 4);
    }

    #[test]
    fn rejects_b0() {
        let mut w = World::new();
        assert!(!w.set_rule(1, 0));
        assert_eq!(w.birth, 1 << 3);
    }

    const TORUS_SIZES: [(i32, i32); 6] = [(8, 8), (16, 30), (64, 64), (66, 130), (126, 62), (128, 192)];

    /// Dense on purpose, so that it shares nothing with the engine and also covers B0.
    fn naive_torus(cells: &Cells, w: i32, h: i32, topology: Topology, birth: u16, survive: u16) -> Cells {
        let n = topology.neighbourhood();
        let mut next = Cells::new();
        for y in 0..h {
            for x in 0..w {
                let offsets = if (x + y) % 2 == 0 { n.even } else { n.odd };
                let count = offsets
                    .iter()
                    .filter(|(dx, dy)| cells.contains(&((x + dx).rem_euclid(w), (y + dy).rem_euclid(h))))
                    .count();
                let mask = if cells.contains(&(x, y)) { survive } else { birth };
                if mask >> count & 1 == 1 {
                    next.insert((x, y));
                }
            }
        }
        next
    }

    /// Ghost chunks hold copies, so only chunks with a population count.
    fn torus_cells(w: &World) -> Cells {
        let mut out = Cells::new();
        for c in w.chunks.iter().filter(|c| c.pop > 0) {
            for (y, &row) in c.rows().iter().enumerate() {
                let mut bits = row;
                while bits != 0 {
                    let x = bits.trailing_zeros() as i32;
                    bits &= bits - 1;
                    out.insert((c.key.0 * 64 + x, c.key.1 * 64 + y as i32));
                }
            }
        }
        out
    }

    fn compare_torus(topology: Topology, birth: u16, survive: u16, density: f64) {
        for (width, height) in TORUS_SIZES {
            let mut w = World::new();
            assert!(w.set_torus(width, height));
            w.set_topology(topology);
            assert!(w.set_rule(birth, survive));
            w.soup(0, 0, width, height, density, 2023 + width as u32);
            let mut reference = torus_cells(&w);
            assert!(!reference.is_empty());
            for generation in 0..40 {
                w.step();
                reference = naive_torus(&reference, width, height, topology, birth, survive);
                let cells = torus_cells(&w);
                assert_eq!(cells, reference, "{topology:?} {width}x{height} step {generation}");
                assert_eq!(w.population as usize, reference.len());
            }
        }
    }

    #[test]
    fn torus_matches_naive_on_every_grid() {
        compare_torus(Topology::Moore, 1 << 3, 1 << 2 | 1 << 3, 0.35);
        compare_torus(Topology::VonNeumann, 1 << 1 | 1 << 3, 1 << 0 | 1 << 2, 0.3);
        compare_torus(Topology::Hexagonal, 1 << 2, 1 << 3 | 1 << 4, 0.3);
        compare_torus(Topology::TriangleEdges, 1 << 1 | 1 << 2, 1 << 1, 0.3);
        compare_torus(Topology::TriangleVertices, 1 << 4, 1 << 4 | 1 << 5 | 1 << 6, 0.3);
    }

    #[test]
    fn torus_runs_birth_from_nothing() {
        compare_torus(Topology::Moore, 1 << 0 | 1 << 3, 1 << 2 | 1 << 3, 0.3);
        compare_torus(Topology::Hexagonal, 1 << 0 | 1 << 4, 1 << 1 | 1 << 2, 0.3);

        let mut w = World::new();
        assert!(w.set_torus(70, 40));
        assert!(w.set_rule(1, 0));
        w.step();
        assert_eq!(w.population, 70 * 40);
        w.step();
        assert_eq!(w.population, 0);
        // A cleared field has to wake up again as well.
        w.step();
        w.clear();
        w.step();
        assert_eq!(w.population, 70 * 40);
    }

    #[test]
    fn torus_keeps_the_skipping_of_quiet_chunks() {
        let mut w = World::new();
        assert!(w.set_torus(256, 256));
        for (x, y) in [(100, 100), (101, 100), (100, 101), (101, 101)] {
            w.set(x, y, true);
        }
        for _ in 0..3 {
            w.step();
        }
        assert_eq!(w.active_chunks, 0);
        // A blinker across the seam must wake the chunks on both sides of it.
        for x in [255, 0, 1] {
            w.set(x, 40, true);
        }
        w.step();
        assert_eq!(torus_cells(&w), [(0, 39), (0, 40), (0, 41), (100, 100), (101, 100), (100, 101), (101, 101)].into());
        w.step();
        assert!(torus_cells(&w).is_superset(&[(255, 40), (0, 40), (1, 40)].into()));
    }

    #[test]
    fn torus_wraps_painting_and_refuses_bad_sizes() {
        let mut w = World::new();
        for (a, b) in [(7, 64), (64, 33), (6, 64), (64, 8194)] {
            assert!(!w.set_torus(a, b), "{a}x{b}");
        }
        assert!(w.torus.is_none());
        assert!(w.set_torus(70, 66));
        w.set(-1, -1, true);
        w.set(70, 66 * 3 + 5, true);
        assert_eq!(torus_cells(&w), [(69, 65), (0, 5)].into());
    }

    #[test]
    fn torus_resizing_keeps_what_fits() {
        let mut w = World::new();
        assert!(w.set_torus(128, 128));
        w.set(3, 4, true);
        w.set(100, 4, true);
        w.step();
        w.set(3, 4, true);
        w.set(4, 4, true);
        w.set(5, 4, true);
        assert!(w.set_torus(64, 64));
        assert_eq!(w.population, 3);
        assert_eq!(w.generation, 0);
        assert_eq!(torus_cells(&w), [(3, 4), (4, 4), (5, 4)].into());
    }

    #[test]
    fn torus_frame_wraps_around_the_edges() {
        let mut w = World::new();
        assert!(w.set_torus(70, 66));
        w.set(69, 0, true);
        w.set(0, 65, true);
        // Starts one column before the east edge and one row before the south edge.
        let f = w.frame(69, 65, 3, 2, 0).to_vec();
        assert_eq!(f, [0, 255, 0, 255, 0, 0]);
        // The same place, a few periods away.
        assert_eq!(w.frame(69 - 140, 65 + 198, 3, 2, 0).to_vec(), f);

        let f = w.frame(0, 0, 35, 33, 1).to_vec();
        let live = f.iter().filter(|&&v| v > 0).count();
        assert_eq!((live, f[34], f[32 * 35]), (2, 63, 63));
    }

    #[test]
    fn export_lists_exactly_the_live_cells() {
        // On a torus this must include no ghosts, even right after a step.
        let mut w = World::new();
        assert!(w.set_torus(70, 66));
        w.soup(0, 0, 70, 66, 0.3, 99);
        w.step();
        let expected = torus_cells(&w);
        let pairs = w.export_cells().to_vec();
        assert_eq!(pairs.len(), expected.len() * 2);
        let listed: Cells = pairs.chunks_exact(2).map(|p| (p[0], p[1])).collect();
        assert_eq!(listed, expected);
    }

    #[test]
    fn frame_rasterises_and_aggregates() {
        let mut w = World::new();
        w.set(-1, -1, true);
        w.set(2, 0, true);
        let f = w.frame(-2, -2, 6, 3, 0).to_vec();
        assert_eq!(f[6 + 1], 255); // (-1, -1)
        assert_eq!(f[2 * 6 + 4], 255); // (2, 0)
        assert_eq!(f.iter().filter(|&&v| v > 0).count(), 2);

        let f = w.frame(-64, -64, 2, 2, 6).to_vec();
        assert_eq!((f[0], f[3]), (1, 1));
    }
}
