//! Life on a tiling with no lattice to exploit: cells are ids on an explicit graph,
//! generated in pieces as activity or the view reaches them. Only cells around a
//! change are looked at, which is what keeps this fast: most of a field does nothing
//! most of the time.
//!
//! What a tiling looks like is up to `Tiling`, see `hat_world.rs` and `penrose.rs`.

use crate::life::KeyHasher;
use crate::status::{Detector, Object, Observed};
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hash};

const MAX_AT_POINT: usize = 8;
/// A limit for memory's sake, at a few hundred bytes per cell: past this, the
/// field simply ends.
const MAX_CELLS: usize = 1_500_000;
pub(crate) const NONE: u32 = u32::MAX;
/// Largest neighbourhood radius, in rings. Its balls must stay below 128 cells,
/// the width of a rule's count mask, which a test checks.
pub const MAX_RADIUS: u8 = 3;

/// Cell bytes double as texture values, so that the renderer can shade states:
/// state 1 is drawn fully, states 2 and 3 progressively lighter.
const STATE_BYTES: [u8; 4] = [0, 255, 150, 90];

fn logical_state(byte: u8) -> usize {
    match byte {
        0 => 0,
        255 => 1,
        150 => 2,
        _ => 3,
    }
}

/// One row of a priority table: the first row whose conditions all hold decides
/// the next state, and no match means state 0. Unlike life-like rules these can
/// see every state, which the known gliders on the hat depend on.
#[derive(Clone)]
struct Row {
    own: Option<u8>,
    /// (state, n): at least n neighbours in that state.
    conditions: Vec<(u8, u8)>,
    next: u8,
}

pub(crate) type Fast = BuildHasherDefault<KeyHasher>;

/// The points and edges of a cell, as the keys that neighbours will share.
pub struct Outline<T: Tiling + ?Sized> {
    pub points: Vec<T::Point>,
    /// This cell's direction of each edge, and the direction its neighbour has.
    pub edges: Vec<(T::Edge, T::Edge)>,
}

/// What the graph needs to know about a tiling.
pub trait Tiling {
    /// A cell's place, enough to tell it from any other.
    type Cell: Copy + PartialEq;
    /// A stretch of the tiling that gets generated as a whole.
    type Piece: Copy + Eq + Hash;
    type Point: Copy + Eq + Hash;
    type Edge: Copy + Eq + Hash;

    /// Most cells that can touch a cell.
    const MAX_NEIGHBOURS: usize;
    /// Distance around a piece that holds everything within one ring of it.
    const RING_MARGIN: f64;

    /// Pieces reaching into a disc. Positions here have no origin subtracted.
    fn pieces_near(&self, x: f64, y: f64, radius: f64, out: &mut Vec<Self::Piece>);
    /// Centre and radius of a piece.
    fn bounds(&self, piece: &Self::Piece) -> ((f64, f64), f64);
    /// The cells of a piece that no other piece has brought yet.
    fn cells_of(&mut self, piece: &Self::Piece, out: &mut Vec<Self::Cell>);
    fn outline(&self, cell: &Self::Cell, out: &mut Outline<Self>);
    /// What is subtracted from all positions that leave the engine.
    fn origin(&self) -> (f64, f64);
    /// Where a cell counts as being, with no origin subtracted.
    fn place(&self, cell: &Self::Cell) -> (f64, f64);
    /// Centre of a cell, with the origin subtracted.
    fn center(&self, cell: &Self::Cell) -> (f64, f64);
    /// A total order that does not depend on generation.
    fn order(&self, cell: &Self::Cell) -> [i64; 6];
    /// A cell's place in the tiling, the same in any world.
    fn key(&self, cell: &Self::Cell) -> CellKey;
    /// The cell a key would be, if any, and one of its points.
    fn from_key(&self, key: CellKey) -> Option<(Self::Cell, Self::Point, (f64, f64))>;
}

/// `rotation + 8 * flip` or the like, then the four parts of a position.
pub type CellKey = [i32; 5];

#[derive(Clone, Copy, Default)]
pub(crate) struct AtPoint {
    pub(crate) cells: [u32; MAX_AT_POINT],
    pub(crate) len: u8,
}

struct Piece {
    center: (f64, f64),
    radius: f64,
    /// Whether everything touching this piece has been generated.
    surrounded: bool,
}

pub struct CellWorld<T: Tiling> {
    pub(crate) tiling: T,

    pub(crate) frames: Vec<T::Cell>,
    piece_of: Vec<u32>,
    pieces: Vec<Piece>,
    known_pieces: HashSet<T::Piece, Fast>,
    pub(crate) at_points: HashMap<T::Point, AtPoint, Fast>,
    /// Edges still waiting for the cell on their other side.
    open_edges: HashMap<T::Edge, u32, Fast>,
    /// Per cell, `T::MAX_NEIGHBOURS` wide: edge neighbours first, then those only
    /// sharing a vertex.
    neighbours: Vec<u32>,
    pub(crate) edge_neighbours: Vec<u8>,
    pub(crate) all_neighbours: Vec<u8>,

    /// For a radius above 1: everything within that many rings, built when a cell
    /// first comes alive, as it needs the field around it to exist.
    balls: Vec<Box<[u32]>>,
    radius: u8,

    pub(crate) state: Vec<u8>,
    /// Live cells in no particular order, and where each cell sits in that list.
    live: Vec<u32>,
    live_pos: Vec<u32>,
    counts: Vec<u8>,
    dirty: Vec<u32>,
    is_dirty: Vec<bool>,
    flips: Vec<(u32, u8)>,
    edges_only: bool,
    table: Option<Vec<Row>>,
    /// One bit per neighbour count.
    birth: u128,
    survive: u128,
    /// The status detector costs a few percent, which a search does not want to pay.
    pub observing: bool,
    changed: bool,
    pub(crate) hash: u64,
    pub generation: u64,
    pub population: u64,
    pub capped: bool,
    pub(crate) max_cells: usize,
    pub detector: Detector,
    pub scratch: Vec<u32>,
    pub(crate) export: Vec<i32>,
}

fn count_range((low, high): (u8, u8)) -> u128 {
    let upper = if high >= 127 { u128::MAX } else { (1u128 << (high + 1)) - 1 };
    upper & !((1u128 << low) - 1)
}

fn mix(id: u32) -> u64 {
    let x = (id as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (x ^ (x >> 29)).wrapping_mul(0xBF58_476D_1CE4_E5B9)
}

/// A cell's share of the field hash, so that it also tells states apart.
fn hash_term(id: u32, byte: u8) -> u64 {
    if byte == 0 { 0 } else { mix(id).wrapping_mul(byte as u64 | 1) }
}

impl<T: Tiling> CellWorld<T> {
    pub(crate) fn with_tiling(tiling: T) -> Self {
        CellWorld {
            tiling,
            frames: Vec::new(),
            piece_of: Vec::new(),
            pieces: Vec::new(),
            known_pieces: HashSet::default(),
            at_points: HashMap::default(),
            open_edges: HashMap::default(),
            neighbours: Vec::new(),
            edge_neighbours: Vec::new(),
            all_neighbours: Vec::new(),
            balls: Vec::new(),
            radius: 1,
            state: Vec::new(),
            live: Vec::new(),
            live_pos: Vec::new(),
            counts: Vec::new(),
            dirty: Vec::new(),
            is_dirty: Vec::new(),
            flips: Vec::new(),
            edges_only: false,
            table: None,
            birth: 1 << 2,
            survive: 1 << 3,
            observing: true,
            changed: false,
            hash: 0,
            generation: 0,
            population: 0,
            capped: false,
            max_cells: MAX_CELLS,
            detector: Detector::default(),
            scratch: Vec::new(),
            export: Vec::new(),
        }
    }

    pub fn cell_count(&self) -> usize {
        self.frames.len()
    }

    pub fn states(&self) -> &[u8] {
        &self.state
    }

    pub fn load_around(&mut self, x: f64, y: f64, radius: f64) {
        let o = self.tiling.origin();
        self.load_absolute(x + o.0, y + o.1, radius);
    }

    pub(crate) fn load_absolute(&mut self, x: f64, y: f64, radius: f64) {
        let mut wanted = Vec::new();
        self.tiling.pieces_near(x, y, radius, &mut wanted);
        for piece in wanted {
            self.add_piece(piece);
        }
    }

    fn add_piece(&mut self, piece: T::Piece) {
        if !self.known_pieces.insert(piece) {
            return;
        }
        if self.frames.len() >= self.max_cells {
            self.capped = true;
            self.known_pieces.remove(&piece);
            return;
        }
        let (center, radius) = self.tiling.bounds(&piece);
        let id = self.pieces.len() as u32;
        self.pieces.push(Piece { center, radius, surrounded: false });

        let mut cells = Vec::new();
        self.tiling.cells_of(&piece, &mut cells);
        let mut outline = Outline { points: Vec::new(), edges: Vec::new() };
        for cell in cells {
            outline.points.clear();
            outline.edges.clear();
            self.tiling.outline(&cell, &mut outline);
            self.add_cell(cell, id, &outline);
        }
    }

    fn add_cell(&mut self, cell: T::Cell, piece: u32, outline: &Outline<T>) {
        let id = self.frames.len() as u32;

        let mut list = vec![NONE; T::MAX_NEIGHBOURS];
        let mut edges = 0;
        for &(forward, backward) in &outline.edges {
            match self.open_edges.remove(&backward) {
                Some(other) => {
                    if !list[..edges].contains(&other) {
                        list[edges] = other;
                        edges += 1;
                    }
                }
                None => {
                    self.open_edges.insert(forward, id);
                }
            }
        }
        let mut all = edges;
        for point in &outline.points {
            let entry = self.at_points.entry(*point).or_default();
            for &other in &entry.cells[..entry.len as usize] {
                if other != id && !list[..all].contains(&other) {
                    list[all] = other;
                    all += 1;
                }
            }
            if !entry.cells[..entry.len as usize].contains(&id) {
                entry.cells[entry.len as usize] = id;
                entry.len += 1;
            }
        }

        self.frames.push(cell);
        self.piece_of.push(piece);
        self.neighbours.resize(self.neighbours.len() + T::MAX_NEIGHBOURS, NONE);
        self.edge_neighbours.push(0);
        self.all_neighbours.push(0);
        self.balls.push(Box::default());
        self.state.push(0);
        self.live_pos.push(NONE);
        self.counts.push(0);
        self.is_dirty.push(false);

        for (index, &other) in list[..all].iter().enumerate() {
            self.link(id, other, index < edges);
            self.link(other, id, index < edges);
        }
    }

    pub(crate) fn row(&self, cell: usize) -> &[u32] {
        &self.neighbours[cell * T::MAX_NEIGHBOURS..(cell + 1) * T::MAX_NEIGHBOURS]
    }

    /// Adds one direction of a connection, keeping edge neighbours in front.
    fn link(&mut self, cell: u32, other: u32, is_edge: bool) {
        let c = cell as usize;
        let all = self.all_neighbours[c] as usize;
        let row = &mut self.neighbours[c * T::MAX_NEIGHBOURS..(c + 1) * T::MAX_NEIGHBOURS];
        if is_edge {
            let edges = self.edge_neighbours[c] as usize;
            row[all] = row[edges];
            row[edges] = other;
            self.edge_neighbours[c] += 1;
        } else {
            row[all] = other;
        }
        self.all_neighbours[c] += 1;
        // With a larger radius, a live cell's whole ball exists before it counts.
        if self.radius == 1 && (is_edge || !self.edges_only) && self.state[other as usize] != 0 {
            self.counts[c] += 1;
            self.mark(cell);
        }
    }

    fn mark(&mut self, cell: u32) {
        if !self.is_dirty[cell as usize] {
            self.is_dirty[cell as usize] = true;
            self.dirty.push(cell);
        }
    }

    pub(crate) fn neighbour_range(&self, cell: usize) -> usize {
        if self.edges_only { self.edge_neighbours[cell] as usize } else { self.all_neighbours[cell] as usize }
    }

    fn set_state(&mut self, cell: u32, byte: u8) {
        let c = cell as usize;
        let old = self.state[c];
        if old == byte {
            return;
        }
        let life_changed = (old == 0) != (byte == 0);
        if life_changed && byte != 0 && self.radius > 1 {
            self.ensure_ball(cell);
        }
        self.state[c] = byte;
        self.hash ^= hash_term(cell, old) ^ hash_term(cell, byte);
        if life_changed {
            if byte != 0 {
                self.population += 1;
                self.live_pos[c] = self.live.len() as u32;
                self.live.push(cell);
            } else {
                self.population -= 1;
                let at = self.live_pos[c] as usize;
                self.live.swap_remove(at);
                if let Some(&moved) = self.live.get(at) {
                    self.live_pos[moved as usize] = at as u32;
                }
                self.live_pos[c] = NONE;
            }
        }
        self.mark(cell);
        if self.radius > 1 {
            let ball = std::mem::take(&mut self.balls[c]);
            for &n in ball.iter() {
                if life_changed {
                    if byte != 0 {
                        self.counts[n as usize] += 1;
                    } else {
                        self.counts[n as usize] -= 1;
                    }
                }
                self.mark(n);
            }
            self.balls[c] = ball;
            return;
        }
        for i in 0..self.neighbour_range(c) {
            let n = self.row(c)[i];
            if life_changed {
                if byte != 0 {
                    self.counts[n as usize] += 1;
                } else {
                    self.counts[n as usize] -= 1;
                }
            }
            self.mark(n);
        }
    }

    fn flip(&mut self, cell: u32) {
        let byte = if self.state[cell as usize] == 0 { 255 } else { 0 };
        self.set_state(cell, byte);
    }

    fn table_next(&self, cell: u32, table: &[Row]) -> u8 {
        let c = cell as usize;
        let mut tally = [0u8; 4];
        for &n in &self.row(c)[..self.neighbour_range(c)] {
            tally[logical_state(self.state[n as usize])] += 1;
        }
        let own = logical_state(self.state[c]) as u8;
        let row = table.iter().find(|row| {
            row.own.is_none_or(|o| o == own)
                && row.conditions.iter().all(|&(state, n)| tally[state as usize] >= n)
        });
        STATE_BYTES[row.map_or(0, |r| r.next as usize)]
    }

    /// Reads a table from the scratch buffer, row by row: own state (255 for any),
    /// number of conditions, the conditions as state and count, next state.
    pub fn set_table_from_scratch(&mut self, n: usize) {
        let data = &self.scratch[..n.min(self.scratch.len())];
        let mut rows = Vec::new();
        let mut i = 0;
        while i + 1 < data.len() {
            let own = data[i];
            let count = data[i + 1] as usize;
            let conditions = data[i + 2..i + 2 + 2 * count]
                .chunks_exact(2)
                .map(|pair| (pair[0] as u8, pair[1] as u8))
                .collect();
            let next = data[i + 2 + 2 * count] as u8;
            rows.push(Row { own: if own == 255 { None } else { Some(own as u8) }, conditions, next });
            i += 3 + 2 * count;
        }
        self.table = Some(rows);
        self.set_radius(1);
        for cell in 0..self.frames.len() as u32 {
            self.mark(cell);
        }
        self.detector.invalidate();
    }

    /// Starts a pattern on a cell and one of its edge neighbours.
    pub fn seed_pair(&mut self, cell: u32, first: u8, second: u8) {
        let c = cell as usize;
        if c >= self.frames.len() {
            return;
        }
        self.set_state(cell, STATE_BYTES[first as usize & 3]);
        self.surround(cell);
        if self.edge_neighbours[c] > 0 {
            let other = self.row(c)[0];
            self.set_state(other, STATE_BYTES[second as usize & 3]);
            self.surround(other);
        }
        self.detector.invalidate();
    }

    /// A live cell may need neighbours that do not exist yet.
    fn surround(&mut self, cell: u32) {
        let piece = self.piece_of[cell as usize] as usize;
        if self.pieces[piece].surrounded {
            return;
        }
        self.pieces[piece].surrounded = true;
        let (center, radius) = (self.pieces[piece].center, self.pieces[piece].radius);
        self.load_absolute(center.0, center.1, radius + T::RING_MARGIN * self.radius as f64);
    }

    fn ensure_ball(&mut self, cell: u32) {
        self.surround(cell);
        if !self.balls[cell as usize].is_empty() {
            return;
        }
        let ball = self.cells_within(cell, self.radius as u32);
        self.balls[cell as usize] = ball.into_boxed_slice();
    }

    /// All cells at 1 to `rings` steps over edges and corners. They must exist.
    pub(crate) fn cells_within(&self, cell: u32, rings: u32) -> Vec<u32> {
        let mut found = vec![cell];
        let mut from = 0;
        for _ in 0..rings {
            let to = found.len();
            for i in from..to {
                let c = found[i] as usize;
                for &n in &self.row(c)[..self.all_neighbours[c] as usize] {
                    if !found.contains(&n) {
                        found.push(n);
                    }
                }
            }
            from = to;
        }
        found.swap_remove(0);
        found
    }

    /// Counts everything anew, for after the meaning of a neighbour has changed.
    fn recount(&mut self) {
        self.counts.iter_mut().for_each(|c| *c = 0);
        for i in 0..self.live.len() {
            let c = self.live[i] as usize;
            if self.radius > 1 {
                self.ensure_ball(c as u32);
                for &n in self.balls[c].iter() {
                    self.counts[n as usize] += 1;
                }
            } else {
                for k in 0..self.neighbour_range(c) {
                    let n = self.row(c)[k] as usize;
                    self.counts[n] += 1;
                }
            }
        }
        for cell in 0..self.frames.len() as u32 {
            self.mark(cell);
        }
    }

    /// How many rings count as neighbours. 1 is the usual neighbourhood.
    fn set_radius(&mut self, radius: u8) {
        if radius == self.radius {
            return;
        }
        self.radius = radius;
        // Both were worked out for the old radius.
        self.pieces.iter_mut().for_each(|p| p.surrounded = false);
        self.balls.iter_mut().for_each(|b| *b = Box::default());
        for i in 0..self.live.len() {
            self.surround(self.live[i]);
        }
        self.recount();
    }

    /// B0 would need every cell of the infinite field to be born, so it is refused.
    pub fn set_rule(&mut self, birth: u16, survive: u16) -> bool {
        if birth & 1 != 0 {
            return false;
        }
        self.apply_rule(birth as u128, survive as u128, 1);
        true
    }

    /// A rule on all cells within `radius` rings: born with `birth.0` to `birth.1`
    /// live cells in there, surviving with `survive.0` to `survive.1`. The cell
    /// itself does not count. Returns false for what the engine cannot do.
    pub fn set_range_rule(&mut self, radius: u8, birth: (u8, u8), survive: (u8, u8)) -> bool {
        let valid = (1..=MAX_RADIUS).contains(&radius)
            && birth.0 >= 1
            && birth.0 <= birth.1
            && birth.1 < 128
            && survive.0 <= survive.1
            && survive.1 < 128;
        if !valid {
            return false;
        }
        self.apply_rule(count_range(birth), count_range(survive), radius);
        true
    }

    fn apply_rule(&mut self, birth: u128, survive: u128, radius: u8) {
        self.birth = birth;
        self.survive = survive;
        self.table = None;
        self.set_radius(radius);
        // A table may have left states behind that life-like rules do not know.
        for i in 0..self.live.len() {
            self.set_state(self.live[i], 255);
        }
        for cell in 0..self.frames.len() as u32 {
            self.mark(cell);
        }
        self.detector.invalidate();
    }

    /// Changes rule and radius of an empty field without visiting every cell, for
    /// searches that try thousands of rules on one world. Masks hold one bit per count.
    pub fn set_masks_when_empty(&mut self, birth: u128, survive: u128, radius: u8) {
        assert!(self.population == 0 && birth & 1 == 0 && (1..=MAX_RADIUS).contains(&radius));
        self.birth = birth;
        self.survive = survive;
        self.table = None;
        self.set_radius(radius);
    }

    /// Empties the field and forgets what was pending, at the cost of what was alive.
    pub fn reset_when_ruled(&mut self) {
        self.clear();
        for i in 0..self.dirty.len() {
            self.is_dirty[self.dirty[i] as usize] = false;
        }
        self.dirty.clear();
        self.hash = 0;
    }

    pub fn set_edges_only(&mut self, edges_only: bool) {
        self.edges_only = edges_only;
        self.recount();
        self.detector.invalidate();
    }

    pub fn clear(&mut self) {
        while let Some(&cell) = self.live.last() {
            self.flip(cell);
        }
        self.generation = 0;
        self.changed = false;
        self.detector.invalidate();
    }

    pub fn paint_scratch(&mut self, n: usize, alive: bool) {
        let ids = std::mem::take(&mut self.scratch);
        for &cell in &ids[..n.min(ids.len())] {
            if (cell as usize) < self.frames.len() && (self.state[cell as usize] != 0) != alive {
                self.flip(cell);
                if alive {
                    self.surround(cell);
                }
            }
        }
        self.scratch = ids;
        self.detector.invalidate();
    }

    pub fn soup(&mut self, x: f64, y: f64, radius: f64, density: f64, seed: u32) {
        self.load_around(x, y, radius);
        let threshold = (density.clamp(0.0, 1.0) * u32::MAX as f64) as u32;
        let mut s = seed | 1;
        let o = self.tiling.origin();
        let (ax, ay) = (x + o.0, y + o.1);
        let mut inside: Vec<u32> = (0..self.frames.len() as u32)
            .filter(|&cell| {
                let at = self.tiling.place(&self.frames[cell as usize]);
                (at.0 - ax).hypot(at.1 - ay) <= radius
            })
            .collect();
        // Ids follow the order of generation, which depends on where the view has
        // been. Going by position, a seed means the same field for everybody.
        inside.sort_unstable_by_key(|&cell| self.tiling.order(&self.frames[cell as usize]));
        for cell in inside {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            if (s < threshold) != (self.state[cell as usize] != 0) {
                self.flip(cell);
                if s < threshold {
                    self.surround(cell);
                }
            }
        }
        self.detector.invalidate();
    }

    pub fn step(&mut self) {
        let dirty = std::mem::take(&mut self.dirty);
        let mut flips = std::mem::take(&mut self.flips);
        flips.clear();
        let table = self.table.take();
        for &cell in &dirty {
            let c = cell as usize;
            self.is_dirty[c] = false;
            let next = match &table {
                Some(rows) => self.table_next(cell, rows),
                None => {
                    let live = self.state[c] != 0;
                    let mask = if live { self.survive } else { self.birth };
                    if mask >> self.counts[c] & 1 == 1 { 255 } else { 0 }
                }
            };
            if next != self.state[c] {
                flips.push((cell, next));
            }
        }
        self.table = table;
        self.dirty = dirty;
        self.dirty.clear();

        for &(cell, next) in &flips {
            self.set_state(cell, next);
            if next != 0 {
                self.surround(cell);
            }
        }
        self.changed = !flips.is_empty();
        self.flips = flips;
        self.generation += 1;

        if self.observing {
            let mut detector = std::mem::take(&mut self.detector);
            detector.observe(&*self);
            self.detector = detector;
        }
    }

    /// Makes a dead cell alive, generating what it needs around it.
    pub fn set_alive(&mut self, cell: u32) {
        if self.state[cell as usize] == 0 {
            self.flip(cell);
            self.surround(cell);
        }
    }

    pub fn live(&self) -> &[u32] {
        &self.live
    }

    pub fn field_hash(&self) -> u64 {
        self.hash
    }

    /// Everything touching a cell, over edges or corners. Parts of it may not exist
    /// yet where the field has not been generated.
    pub fn touching(&self, cell: u32) -> &[u32] {
        &self.row(cell as usize)[..self.all_neighbours[cell as usize] as usize]
    }

    pub fn set_max_cells(&mut self, max_cells: usize) {
        self.max_cells = max_cells;
    }

    /// Centre of a cell, in the units of `load_around`.
    pub fn center(&self, cell: u32) -> (f64, f64) {
        self.tiling.center(&self.frames[cell as usize])
    }

    /// A cell's place in the tiling, the five numbers of `export_cells`. Unlike its
    /// id, which depends on the order of generation, it means the same in any world.
    pub fn key_of(&self, cell: u32) -> CellKey {
        self.tiling.key(&self.frames[cell as usize])
    }

    /// Generates the field where needed. None if the tiling has no such cell.
    pub fn cell_of(&mut self, key: CellKey) -> Option<u32> {
        let (cell, point, place) = self.tiling.from_key(key)?;
        self.load_absolute(place.0, place.1, 1.0);
        let entry = self.at_points.get(&point)?;
        entry.cells[..entry.len as usize].iter().copied().find(|&c| self.frames[c as usize] == cell)
    }

    /// Sets cells given by flat keys in the scratch buffer. Returns how many of them
    /// the tiling does not have.
    pub fn paint_keys_from_scratch(&mut self, n: usize, alive: bool) -> u32 {
        let data = std::mem::take(&mut self.scratch);
        let mut missing = 0;
        for key in data[..n.min(data.len())].chunks_exact(5) {
            let key = [key[0] as i32, key[1] as i32, key[2] as i32, key[3] as i32, key[4] as i32];
            match self.cell_of(key) {
                Some(cell) => {
                    if (self.state[cell as usize] != 0) != alive {
                        self.flip(cell);
                        if alive {
                            self.surround(cell);
                        }
                    }
                }
                None => missing += 1,
            }
        }
        self.scratch = data;
        self.detector.invalidate();
        missing
    }

    /// The live cells as flat keys.
    pub fn export_live(&mut self) -> &[i32] {
        let mut out = std::mem::take(&mut self.export);
        out.clear();
        for &cell in &self.live {
            out.extend(self.key_of(cell));
        }
        self.export = out;
        &self.export
    }

    pub fn export_cells(&mut self, from: usize) -> &[i32] {
        let mut out = std::mem::take(&mut self.export);
        out.clear();
        for cell in &self.frames[from.min(self.frames.len())..] {
            out.extend(self.tiling.key(cell));
        }
        self.export = out;
        &self.export
    }
}

impl<T: Tiling> Observed for CellWorld<T> {
    fn generation(&self) -> u64 {
        self.generation
    }
    fn population(&self) -> u64 {
        self.population
    }
    fn any_changed(&self) -> bool {
        self.changed
    }
    fn field_hash(&self) -> u64 {
        self.hash
    }
    fn objects(&self) -> Option<(Vec<Object>, crate::life::Topology)> {
        None
    }
}
