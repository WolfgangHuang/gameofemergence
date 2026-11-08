//! Life on the hat tiling. There is no lattice to exploit, so cells are ids on an
//! explicit graph, generated in pieces as activity or the view reaches them. Only
//! cells around a change are looked at, which is what keeps this fast: most of a
//! field does nothing most of the time.

use crate::hat::{Frame, Hex, Hierarchy, LEVELS, Split, directed_edge, hat_points};
use crate::life::KeyHasher;
use crate::status::{Detector, Object, Observed};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;

const ROOT_LEVEL: usize = LEVELS - 1;
const ROOT_KIND: usize = 0;
/// Pieces are level 2 metatiles, which hold up to 169 hats.
const PIECE_LEVEL: usize = 2;
const MAX_NEIGHBOURS: usize = 10;
const MAX_AT_POINT: usize = 6;
/// A limit for memory's sake, at a few hundred bytes per cell: past this, the
/// field simply ends.
const MAX_CELLS: usize = 1_500_000;
const NONE: u32 = u32::MAX;
/// Largest neighbourhood radius, in rings. Its balls must stay below 128 cells,
/// the width of a rule's count mask, which a test checks.
pub const MAX_RADIUS: u8 = 3;
/// Lattice units around a piece that hold everything within one ring of it: more
/// than the longest diameter of a hat.
const RING_MARGIN: f64 = 8.0;

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

type Fast = BuildHasherDefault<KeyHasher>;

fn point_key(p: Hex) -> u64 {
    ((p.q as u32 as u64) << 32) | p.r as u32 as u64
}

/// Lattice coordinates stay well within 26 bits and edges are short, so one word
/// holds an edge: start point, then the offset to its end point.
fn edge_key(from: Hex, to: Hex) -> u64 {
    let (dq, dr) = ((to.q - from.q + 4) as u64, (to.r - from.r + 4) as u64);
    (((from.q + (1 << 25)) as u64) << 38) | (((from.r + (1 << 25)) as u64) << 12) | (dq << 6) | dr
}

#[derive(Clone, Copy, Default)]
struct AtPoint {
    cells: [u32; MAX_AT_POINT],
    len: u8,
}

struct Piece {
    center: (f64, f64),
    radius: f64,
    /// Whether everything touching this piece has been generated.
    surrounded: bool,
}

pub struct HatWorld {
    hierarchy: Hierarchy,
    root: Frame,
    /// Split position of the hat nearest the origin, subtracted from all output.
    origin: Split,

    frames: Vec<Frame>,
    piece_of: Vec<u32>,
    pieces: Vec<Piece>,
    known_pieces: HashSet<Frame, Fast>,
    at_points: HashMap<u64, AtPoint, Fast>,
    /// Edges still waiting for the hat on their other side.
    open_edges: HashMap<u64, u32, Fast>,
    /// Edge neighbours first, then those only sharing a vertex.
    neighbours: Vec<[u32; MAX_NEIGHBOURS]>,
    edge_neighbours: Vec<u8>,
    all_neighbours: Vec<u8>,

    /// H, T, P or F metatile around each cell, see `metatile`.
    metatile: Vec<u8>,
    /// For a radius above 1: everything within that many rings, built when a cell
    /// first comes alive, as it needs the field around it to exist.
    balls: Vec<Box<[u32]>>,
    radius: u8,

    state: Vec<u8>,
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
    /// Centre of a hat relative to its position, by `rotation + 8 * flip`.
    center_offsets: [(f64, f64); 16],
    changed: bool,
    hash: u64,
    pub generation: u64,
    pub population: u64,
    pub capped: bool,
    max_cells: usize,
    pub detector: Detector,
    pub scratch: Vec<u32>,
    export: Vec<i32>,
    ratio: (f64, f64),
}

/// A hat's place in the tiling: `rotation + 8 * flip`, then the two parts of its position.
pub type CellKey = [i32; 5];

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

fn center_of(frame: &Frame, p: (f64, f64)) -> (f64, f64) {
    let y = if frame.flip { -p.1 } else { p.1 };
    let (s, c) = (frame.rotation as f64 * std::f64::consts::PI / 3.0).sin_cos();
    let at = frame.at.lattice();
    (c * p.0 - s * y + at.x(), s * p.0 + c * y + at.y())
}

impl Default for HatWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl HatWorld {
    pub fn new() -> Self {
        let hierarchy = Hierarchy::new();
        let proto = &hierarchy.levels[ROOT_LEVEL][ROOT_KIND];
        // Puts the middle of the huge root metatile at the origin.
        let shift = Hex::new(
            -(proto.center.0 - proto.center.1 / (2.0 * 0.866_025_403_784_438_6)).round() as i64,
            -(proto.center.1 / 0.866_025_403_784_438_6).round() as i64,
        );
        let root = Frame { rotation: 0, flip: false, at: Split { p: shift, q: Hex::default() } };

        // The distance of the far children is long enough to average over the
        // tiling's irregularities.
        let far = hierarchy.levels[10][ROOT_KIND]
            .children
            .iter()
            .max_by_key(|c| c.frame.at.p.q.abs() + c.frame.at.p.r.abs())
            .expect("metatiles have children")
            .frame
            .at;
        let (px, py, qx, qy) = (far.p.x(), far.p.y(), far.q.x(), far.q.y());
        let norm = px * px + py * py;
        let ratio = ((qx * px + qy * py) / norm, (qy * px - qx * py) / norm);

        let (hat_center, _) = hierarchy.hat_bounds();
        let mut center_offsets = [(0.0, 0.0); 16];
        for (index, offset) in center_offsets.iter_mut().enumerate() {
            let frame = Frame { rotation: (index & 7) as u8 % 6, flip: index >= 8, at: Split::default() };
            *offset = center_of(&frame, hat_center);
        }

        let mut world = HatWorld {
            hierarchy,
            root,
            origin: Split::default(),
            frames: Vec::new(),
            piece_of: Vec::new(),
            pieces: Vec::new(),
            known_pieces: HashSet::default(),
            at_points: HashMap::default(),
            open_edges: HashMap::default(),
            neighbours: Vec::new(),
            edge_neighbours: Vec::new(),
            all_neighbours: Vec::new(),
            metatile: Vec::new(),
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
            center_offsets,
            changed: false,
            hash: 0,
            generation: 0,
            population: 0,
            capped: false,
            max_cells: MAX_CELLS,
            detector: Detector::default(),
            scratch: Vec::new(),
            export: Vec::new(),
            ratio,
        };

        // The hat closest to the middle defines the origin of all output, so that
        // long distances there grow in the ratio that `ratio` describes.
        world.load_absolute(0.0, 0.0, 12.0);
        let nearest = world
            .frames
            .iter()
            .min_by_key(|f| {
                let at = f.at.lattice();
                at.q * at.q + at.q * at.r + at.r * at.r
            })
            .copied()
            .expect("the middle of the tiling is not empty");
        world.origin = nearest.at;
        world
    }

    pub fn ratio(&self) -> (f64, f64) {
        self.ratio
    }

    pub fn cell_count(&self) -> usize {
        self.frames.len()
    }

    pub fn states(&self) -> &[u8] {
        &self.state
    }

    pub fn load_around(&mut self, x: f64, y: f64, radius: f64) {
        let o = self.origin.lattice();
        self.load_absolute(x + o.x(), y + o.y(), radius);
    }

    fn load_absolute(&mut self, x: f64, y: f64, radius: f64) {
        let mut stack = vec![(ROOT_LEVEL, ROOT_KIND, self.root)];
        let mut wanted = Vec::new();
        while let Some((level, kind, frame)) = stack.pop() {
            let proto = &self.hierarchy.levels[level][kind];
            let c = center_of(&frame, proto.center);
            if (c.0 - x).hypot(c.1 - y) > radius + proto.radius {
                continue;
            }
            if level == PIECE_LEVEL {
                wanted.push((kind, frame));
                continue;
            }
            for child in &proto.children {
                stack.push((level - 1, child.kind, frame.then(&child.frame)));
            }
        }
        for (kind, frame) in wanted {
            self.add_piece(kind, frame);
        }
    }

    fn add_piece(&mut self, kind: usize, frame: Frame) {
        if !self.known_pieces.insert(frame) {
            return;
        }
        if self.frames.len() >= self.max_cells {
            self.capped = true;
            self.known_pieces.remove(&frame);
            return;
        }
        let proto = &self.hierarchy.levels[PIECE_LEVEL][kind];
        let center = center_of(&frame, proto.center);
        let piece = self.pieces.len() as u32;
        self.pieces.push(Piece { center, radius: proto.radius, surrounded: false });

        let mut hats = Vec::with_capacity(proto.hats as usize);
        crate::hat::for_each_hat_tagged(&self.hierarchy, PIECE_LEVEL, kind, &frame, &mut |f, metatile| hats.push((*f, metatile as u8)));
        for (hat, metatile) in hats {
            self.add_cell(hat, piece, metatile);
        }
    }

    fn add_cell(&mut self, frame: Frame, piece: u32, metatile: u8) {
        let id = self.frames.len() as u32;
        let points = hat_points(&self.hierarchy, &frame);

        let mut edge_list = [NONE; MAX_NEIGHBOURS];
        let mut edges = 0;
        for k in 0..14 {
            let (from, to) = directed_edge(&frame, &points, k);
            match self.open_edges.remove(&edge_key(to, from)) {
                Some(other) => {
                    if !edge_list[..edges].contains(&other) {
                        edge_list[edges] = other;
                        edges += 1;
                    }
                }
                None => {
                    self.open_edges.insert(edge_key(from, to), id);
                }
            }
        }
        let mut list = edge_list;
        let mut all = edges;
        for point in &points {
            let entry = self.at_points.entry(point_key(*point)).or_default();
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

        self.frames.push(frame);
        self.piece_of.push(piece);
        self.neighbours.push([NONE; MAX_NEIGHBOURS]);
        self.edge_neighbours.push(0);
        self.all_neighbours.push(0);
        self.metatile.push(metatile);
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

    /// Adds one direction of a connection, keeping edge neighbours in front.
    fn link(&mut self, cell: u32, other: u32, is_edge: bool) {
        let c = cell as usize;
        let all = self.all_neighbours[c] as usize;
        let row = &mut self.neighbours[c];
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

    fn neighbour_range(&self, cell: usize) -> usize {
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
            let n = self.neighbours[c][i];
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
        for &n in &self.neighbours[c][..self.neighbour_range(c)] {
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
            let other = self.neighbours[c][0];
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
        self.load_absolute(center.0, center.1, radius + RING_MARGIN * self.radius as f64);
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
    fn cells_within(&self, cell: u32, rings: u32) -> Vec<u32> {
        let mut found = vec![cell];
        let mut from = 0;
        for _ in 0..rings {
            let to = found.len();
            for i in from..to {
                let c = found[i] as usize;
                for &n in &self.neighbours[c][..self.all_neighbours[c] as usize] {
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
                    let n = self.neighbours[c][k] as usize;
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
        let o = self.origin.lattice();
        let (ax, ay) = (x + o.x(), y + o.y());
        let mut inside: Vec<u32> = (0..self.frames.len() as u32)
            .filter(|&cell| {
                let at = self.frames[cell as usize].at.lattice();
                (at.x() - ax).hypot(at.y() - ay) <= radius
            })
            .collect();
        // Ids follow the order of generation, which depends on where the view has
        // been. Going by position, a seed means the same field for everybody.
        inside.sort_unstable_by_key(|&cell| {
            let f = &self.frames[cell as usize];
            (f.at.p.q, f.at.p.r, f.at.q.q, f.at.q.r, f.rotation, f.flip)
        });
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
        &self.neighbours[cell as usize][..self.all_neighbours[cell as usize] as usize]
    }

    /// 0 to 3 for H, T, P and F.
    pub fn metatile(&self, cell: u32) -> u8 {
        self.metatile[cell as usize]
    }

    /// A mirrored hat. Each H metatile holds one, and nothing else does.
    pub fn is_mirrored(&self, cell: u32) -> bool {
        self.frames[cell as usize].flip
    }

    pub fn orientation(&self, cell: u32) -> u8 {
        self.frames[cell as usize].rotation
    }

    pub fn set_max_cells(&mut self, max_cells: usize) {
        self.max_cells = max_cells;
    }

    /// Centre of a hat, in the lattice units of `load_around`.
    pub fn center(&self, cell: u32) -> (f64, f64) {
        let frame = &self.frames[cell as usize];
        let at = frame.at.lattice();
        let origin = self.origin.lattice();
        let offset = self.center_offsets[frame.rotation as usize + if frame.flip { 8 } else { 0 }];
        ((at.x() - origin.x()) + offset.0, (at.y() - origin.y()) + offset.1)
    }

    /// A cell's place in the tiling, the five numbers of `export_cells`. Unlike its
    /// id, which depends on the order of generation, it means the same in any world.
    pub fn key_of(&self, cell: u32) -> CellKey {
        let frame = &self.frames[cell as usize];
        [
            frame.rotation as i32 + if frame.flip { 8 } else { 0 },
            (frame.at.p.q - self.origin.p.q) as i32,
            (frame.at.p.r - self.origin.p.r) as i32,
            (frame.at.q.q - self.origin.q.q) as i32,
            (frame.at.q.r - self.origin.q.r) as i32,
        ]
    }

    /// Generates the field where needed. None if the tiling has no such hat.
    pub fn cell_of(&mut self, key: CellKey) -> Option<u32> {
        let rotation = key[0] & 7;
        let frame = Frame {
            rotation: rotation as u8,
            flip: key[0] & 8 != 0,
            at: Split {
                p: Hex::new(key[1] as i64 + self.origin.p.q, key[2] as i64 + self.origin.p.r),
                q: Hex::new(key[3] as i64 + self.origin.q.q, key[4] as i64 + self.origin.q.r),
            },
        };
        if !(0..16).contains(&key[0]) || rotation >= 6 {
            return None;
        }
        let at = frame.at.lattice();
        self.load_absolute(at.x(), at.y(), 1.0);
        let entry = self.at_points.get(&point_key(at))?;
        entry.cells[..entry.len as usize].iter().copied().find(|&c| self.frames[c as usize] == frame)
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
        self.export.clear();
        for frame in &self.frames[from.min(self.frames.len())..] {
            let at = Split {
                p: Hex::new(frame.at.p.q - self.origin.p.q, frame.at.p.r - self.origin.p.r),
                q: Hex::new(frame.at.q.q - self.origin.q.q, frame.at.q.r - self.origin.q.r),
            };
            self.export.extend([
                frame.rotation as i32 + if frame.flip { 8 } else { 0 },
                at.p.q as i32,
                at.p.r as i32,
                at.q.q as i32,
                at.q.r as i32,
            ]);
        }
        &self.export
    }

    pub fn export_outline(&mut self) -> &[i32] {
        self.export.clear();
        for v in &self.hierarchy.outline {
            self.export.extend([v.p.q as i32, v.p.r as i32, v.q.q as i32, v.q.r as i32]);
        }
        &self.export
    }
}

impl Observed for HatWorld {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::Status;
    use std::collections::HashSet as Set;

    fn live(w: &HatWorld) -> Vec<u32> {
        (0..w.frames.len() as u32).filter(|&c| w.state[c as usize] != 0).collect()
    }

    /// Recomputes the next generation from scratch, ignoring all bookkeeping.
    fn naive_next(w: &HatWorld, birth: u16, survive: u16) -> Set<u32> {
        let mut next = Set::new();
        for c in 0..w.frames.len() {
            let range = w.neighbour_range(c);
            let n = w.neighbours[c][..range].iter().filter(|&&o| w.state[o as usize] != 0).count();
            let mask = if w.state[c] != 0 { survive } else { birth };
            if mask >> n & 1 == 1 {
                next.insert(c as u32);
            }
        }
        next
    }

    /// The same with everything within `radius` rings, which the field must contain.
    fn naive_range_next(w: &HatWorld, radius: u32, birth: (u8, u8), survive: (u8, u8)) -> Set<u32> {
        let mut next = Set::new();
        let mut near = Set::new();
        for &live in w.live() {
            near.insert(live);
            near.extend(w.cells_within(live, radius));
        }
        for &c in &near {
            let ball = w.cells_within(c, radius);
            let n = ball.iter().filter(|&&o| w.state[o as usize] != 0).count() as u8;
            let (low, high) = if w.state[c as usize] != 0 { survive } else { birth };
            if low <= n && n <= high {
                next.insert(c);
            }
        }
        next
    }

    #[test]
    fn range_rules_match_a_full_recount() {
        for (radius, birth, survive, seed) in [(2, (3, 5), (2, 7), 11), (2, (4, 6), (3, 9), 12), (3, (9, 13), (6, 14), 13)] {
            let mut w = HatWorld::new();
            assert!(w.set_range_rule(radius, birth, survive));
            // Generated around the action in advance, so that the reference sees all
            // it needs, then compared against what generating on demand would do.
            w.load_around(0.0, 0.0, 700.0);
            w.soup(0.0, 0.0, 25.0, 0.4, seed);
            for generation in 0..25 {
                let expected = naive_range_next(&w, radius as u32, birth, survive);
                w.step();
                let actual: Set<u32> = live(&w).into_iter().collect();
                assert_eq!(actual, expected, "radius {radius} generation {generation}");
            }
        }
    }

    /// The field generated on demand must behave like one that was there all along,
    /// which is what makes a measurement free of edge effects.
    #[test]
    fn generating_on_demand_changes_nothing_at_any_radius() {
        for radius in 1..=MAX_RADIUS {
            let mut lazy = HatWorld::new();
            let mut full = HatWorld::new();
            full.load_around(0.0, 0.0, 400.0);
            let birth = (radius * 3 - 1, radius * 4);
            let survive = (radius * 2, radius * 5);
            assert!(lazy.set_range_rule(radius, birth, survive));
            assert!(full.set_range_rule(radius, birth, survive));
            lazy.soup(0.0, 0.0, 20.0, 0.45, 99);
            full.soup(0.0, 0.0, 20.0, 0.45, 99);
            for generation in 0..60 {
                let keys = |w: &HatWorld| -> Set<CellKey> { w.live().iter().map(|&c| w.key_of(c)).collect() };
                assert_eq!(keys(&lazy), keys(&full), "radius {radius} generation {generation}");
                lazy.step();
                full.step();
            }
        }
    }

    #[test]
    fn balls_fit_the_count_mask() {
        let mut w = HatWorld::new();
        w.load_around(0.0, 0.0, 200.0);
        let center: Vec<u32> = (0..w.frames.len() as u32).filter(|&c| { let p = w.center(c); p.0.hypot(p.1) < 60.0 }).collect();
        for &c in &center {
            assert!(w.cells_within(c, MAX_RADIUS as u32).len() < 128);
        }
    }

    #[test]
    fn keys_mean_the_same_in_any_world() {
        let mut first = HatWorld::new();
        first.load_around(300.0, -200.0, 30.0);
        let cells: Vec<u32> = (0..first.frames.len() as u32).step_by(37).collect();
        let keys: Vec<CellKey> = cells.iter().map(|&c| first.key_of(c)).collect();
        let mut other = HatWorld::new();
        for (key, &cell) in keys.iter().zip(&cells) {
            let there = other.cell_of(*key).expect("the hat exists");
            assert_eq!(other.key_of(there), *key);
            assert_eq!(other.center(there), first.center(cell));
        }
    }

    #[test]
    fn keys_of_no_hat_are_refused() {
        let mut w = HatWorld::new();
        assert_eq!(w.cell_of([0, 1, 1, 1, 1]).is_some(), false);
        assert_eq!(w.cell_of([7, 0, 0, 0, 0]), None);
        assert_eq!(w.cell_of([-1, 0, 0, 0, 0]), None);
        assert_eq!(w.cell_of([0, i32::MAX, i32::MIN, i32::MAX, i32::MIN]), None);
    }

    #[test]
    fn painting_by_key_and_exporting_round_trips() {
        let mut w = HatWorld::new();
        let keys: Vec<CellKey> = (0..6).map(|c| w.key_of(c * 5)).collect();
        w.scratch = keys.iter().flatten().map(|&v| v as u32).collect();
        let n = w.scratch.len();
        assert_eq!(w.paint_keys_from_scratch(n, true), 0);
        assert_eq!(w.population, 6);
        let mut exported: Vec<CellKey> = w.export_live().chunks_exact(5).map(|k| [k[0], k[1], k[2], k[3], k[4]]).collect();
        exported.sort();
        let mut expected = keys.clone();
        expected.sort();
        assert_eq!(exported, expected);
    }

    #[test]
    fn mirrored_hats_sit_in_h_metatiles_only() {
        let mut w = HatWorld::new();
        w.load_around(0.0, 0.0, 300.0);
        let mirrored = (0..w.frames.len() as u32).filter(|&c| w.is_mirrored(c)).count();
        assert!(mirrored > 0);
        assert!((0..w.frames.len() as u32).filter(|&c| w.is_mirrored(c)).all(|c| w.metatile(c) == 0));
        for kind in 0..4 {
            assert!((0..w.frames.len() as u32).any(|c| w.metatile(c) == kind), "kind {kind}");
        }
    }

    #[test]
    fn the_live_list_follows_the_field() {
        let mut w = HatWorld::new();
        assert!(w.set_rule(1 << 2 | 1 << 5, 1 << 3));
        w.soup(0.0, 0.0, 40.0, 0.4, 5);
        for _ in 0..60 {
            w.step();
            let mut listed: Vec<u32> = w.live().to_vec();
            listed.sort_unstable();
            assert_eq!(listed, live(&w));
        }
        w.clear();
        assert!(w.live().is_empty());
        assert_eq!(w.field_hash(), 0);
    }

    /// Adjacency worked out directly from shared points, in a small patch.
    #[test]
    fn links_match_geometry_and_are_symmetric() {
        let mut w = HatWorld::new();
        w.load_around(0.0, 0.0, 120.0);
        let mut by_point: HashMap<Hex, Vec<u32>> = HashMap::new();
        let mut by_edge: HashMap<(Hex, Hex), u32> = HashMap::new();
        for (i, f) in w.frames.iter().enumerate() {
            let points = hat_points(&w.hierarchy, f);
            for k in 0..14 {
                by_point.entry(points[k]).or_default().push(i as u32);
                by_edge.insert(directed_edge(f, &points, k), i as u32);
            }
        }
        for (i, f) in w.frames.iter().enumerate() {
            let points = hat_points(&w.hierarchy, f);
            let mut vertex: Set<u32> = Set::new();
            let mut edge: Set<u32> = Set::new();
            for k in 0..14 {
                vertex.extend(by_point[&points[k]].iter().copied());
                let (a, b) = directed_edge(f, &points, k);
                if let Some(&o) = by_edge.get(&(b, a)) {
                    edge.insert(o);
                }
            }
            vertex.remove(&(i as u32));
            let all: Set<u32> = w.neighbours[i][..w.all_neighbours[i] as usize].iter().copied().collect();
            let front: Set<u32> = w.neighbours[i][..w.edge_neighbours[i] as usize].iter().copied().collect();
            assert_eq!(all, vertex, "cell {i}");
            assert_eq!(front, edge, "cell {i}");
            for o in &all {
                assert!(w.neighbours[*o as usize].contains(&(i as u32)));
            }
        }
    }

    fn agrees_with_naive(edges_only: bool, birth: u16, survive: u16, seed: u32) {
        let mut w = HatWorld::new();
        w.set_edges_only(edges_only);
        assert!(w.set_rule(birth, survive));
        w.soup(0.0, 0.0, 60.0, 0.35, seed);
        for generation in 0..80 {
            let expected = naive_next(&w, birth, survive);
            w.step();
            let actual: Set<u32> = live(&w).into_iter().collect();
            assert_eq!(actual, expected, "generation {generation}");
            assert_eq!(w.population as usize, expected.len());
        }
    }

    #[test]
    fn a_seed_means_the_same_field_wherever_the_view_has_been() {
        let cells = |w: &HatWorld| -> Set<(i64, i64, i64, i64, u8, bool)> {
            live(w)
                .into_iter()
                .map(|c| {
                    let f = &w.frames[c as usize];
                    (f.at.p.q, f.at.p.r, f.at.q.q, f.at.q.r, f.rotation, f.flip)
                })
                .collect()
        };
        let mut direct = HatWorld::new();
        direct.soup(10.0, -5.0, 40.0, 0.3, 4711);
        let mut wandered = HatWorld::new();
        wandered.load_around(90.0, 70.0, 30.0);
        wandered.load_around(-60.0, 20.0, 45.0);
        wandered.soup(10.0, -5.0, 40.0, 0.3, 4711);
        assert_ne!(direct.frames.len(), wandered.frames.len());
        assert!(direct.population > 0);
        assert_eq!(cells(&direct), cells(&wandered));
    }

    #[test]
    fn matches_a_full_recount_with_all_neighbours() {
        agrees_with_naive(false, 1 << 2, 1 << 3 | 1 << 4, 12345);
    }

    #[test]
    fn matches_a_full_recount_with_edge_neighbours() {
        agrees_with_naive(true, 1 << 2 | 1 << 4, 1 << 3, 54321);
    }

    /// A rule that fills everything must end at the cell limit, not out of memory.
    #[test]
    fn explosive_growth_stops_at_the_limit() {
        let mut w = HatWorld::new();
        w.max_cells = 30_000;
        assert!(w.set_rule(1 << 1, (1 << 7) - 1));
        w.soup(0.0, 0.0, 20.0, 0.5, 3);
        while !w.capped {
            w.step();
            assert!(w.generation < 5000, "only {} cells so far", w.cell_count());
        }
        assert!(w.cell_count() <= w.max_cells + 200);
        for _ in 0..3 {
            w.step();
        }
    }

    fn glider_a() -> HatWorld {
        // James Edmond's hat glider A, verbatim from results/hat-tableevolve-r48-s21.jsonl
        // of monotile-ca, written in the layout of `set_table_from_scratch`.
        let mut w = HatWorld::new();
        w.scratch = vec![
            0, 1, 2, 2, 0,
            255, 2, 1, 3, 1, 1, 3,
            2, 2, 1, 1, 0, 1, 1,
            1, 2, 2, 1, 2, 1, 3,
            2, 0, 0,
            0, 1, 1, 1, 2,
            2, 1, 3, 2, 0,
            2, 0, 0,
        ];
        let n = w.scratch.len();
        w.set_table_from_scratch(n);
        w
    }

    /// Long flights are checked by scripts/check-hat-glider.mjs.
    #[test]
    fn glider_a_stays_small_and_generates_its_way() {
        let mut w = glider_a();
        let before = w.cell_count();
        w.seed_pair(0, 1, 2);
        for _ in 0..300 {
            w.step();
            assert!(w.population <= 16, "population {}", w.population);
        }
        assert!(w.population > 0);
        assert!(w.cell_count() > before);
    }

    #[test]
    fn leaving_a_table_restores_a_consistent_field() {
        let mut w = glider_a();
        w.seed_pair(0, 1, 2);
        for _ in 0..20 {
            w.step();
        }
        assert!(w.set_rule(1 << 2 | 1 << 5, 1 << 3));
        let expected = naive_next(&w, 1 << 2 | 1 << 5, 1 << 3);
        w.step();
        let actual: Set<u32> = live(&w).into_iter().collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn activity_generates_the_field_ahead_of_it() {
        let mut w = HatWorld::new();
        let before = w.cell_count();
        assert!(w.set_rule(1 << 2, 1 << 3 | 1 << 4));
        w.soup(0.0, 0.0, 30.0, 0.5, 7);
        for _ in 0..50 {
            w.step();
        }
        assert!(w.cell_count() > before);
    }

    #[test]
    fn distant_regions_load_without_a_chain_and_agree_on_splits() {
        let mut w = HatWorld::new();
        let origin = w.cell_count();
        w.load_around(500_000.0, -300_000.0, 40.0);
        assert!(w.cell_count() > origin);
        // The far patch has its own hats, whose splits must fit the ratio estimate.
        let (re, im) = w.ratio();
        let far = w.frames[w.frames.len() - 1].at;
        let (px, py, qx, qy) = (
            (far.p.q - w.origin.p.q) as f64 + 0.5 * (far.p.r - w.origin.p.r) as f64,
            0.866_025_403_784_438_6 * (far.p.r - w.origin.p.r) as f64,
            (far.q.q - w.origin.q.q) as f64 + 0.5 * (far.q.r - w.origin.q.r) as f64,
            0.866_025_403_784_438_6 * (far.q.r - w.origin.q.r) as f64,
        );
        let (ex, ey) = (re * px - im * py, re * py + im * px);
        let relative = (qx - ex).hypot(qy - ey) / qx.hypot(qy);
        assert!(relative < 0.01, "split ratio off by {relative}");
    }

    #[test]
    fn oscillation_is_recognised_or_nothing_is_claimed_falsely() {
        // Any claim of a settled state must keep holding.
        let bits = |counts: &[u32]| counts.iter().fold(0u16, |m, c| m | 1 << c);
        let mut settled = 0;
        for (birth, survive) in [(bits(&[3]), bits(&[2, 3])), (bits(&[3, 4]), bits(&[3])), (bits(&[3]), bits(&[2, 3, 4]))] {
            for seed in 1..=4 {
                let mut w = HatWorld::new();
                assert!(w.set_rule(birth, survive));
                w.soup(0.0, 0.0, 40.0, 0.3, seed * 7919);
                for _ in 0..3000 {
                    w.step();
                    if !matches!(w.detector.status, Status::Active | Status::Growing { .. }) {
                        break;
                    }
                }
                match w.detector.status {
                    Status::Oscillating { period } => {
                        settled += 1;
                        let hash = w.hash;
                        for _ in 0..period * 10 {
                            w.step();
                        }
                        assert_eq!(w.hash, hash);
                    }
                    Status::Still | Status::Empty => {
                        settled += 1;
                        let hash = w.hash;
                        for _ in 0..20 {
                            w.step();
                        }
                        assert_eq!(w.hash, hash);
                    }
                    Status::Drifting { .. } => panic!("the detector has no way to prove drift on this tiling"),
                    _ => {}
                }
            }
        }
        assert!(settled > 0, "no case settled");
    }
}
