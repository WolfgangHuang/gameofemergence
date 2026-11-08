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

    state: Vec<u8>,
    counts: Vec<u8>,
    dirty: Vec<u32>,
    is_dirty: Vec<bool>,
    flips: Vec<(u32, u8)>,
    edges_only: bool,
    table: Option<Vec<Row>>,
    birth: u16,
    survive: u16,
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
            state: Vec::new(),
            counts: Vec::new(),
            dirty: Vec::new(),
            is_dirty: Vec::new(),
            flips: Vec::new(),
            edges_only: false,
            table: None,
            birth: 1 << 2,
            survive: 1 << 3,
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
        crate::hat::for_each_hat(&self.hierarchy, PIECE_LEVEL, kind, &frame, &mut |f| hats.push(*f));
        for hat in hats {
            self.add_cell(hat, piece);
        }
    }

    fn add_cell(&mut self, frame: Frame, piece: u32) {
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
        self.state.push(0);
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
        if (is_edge || !self.edges_only) && self.state[other as usize] != 0 {
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
        self.state[c] = byte;
        self.hash ^= hash_term(cell, old) ^ hash_term(cell, byte);
        let life_changed = (old == 0) != (byte == 0);
        if life_changed {
            if byte != 0 {
                self.population += 1;
            } else {
                self.population -= 1;
            }
        }
        self.mark(cell);
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
        self.load_absolute(center.0, center.1, radius + 8.0);
    }

    /// B0 would need every cell of the infinite field to be born, so it is refused.
    pub fn set_rule(&mut self, birth: u16, survive: u16) -> bool {
        if birth & 1 != 0 {
            return false;
        }
        self.birth = birth;
        self.survive = survive;
        self.table = None;
        // A table may have left states behind that life-like rules do not know.
        for cell in 0..self.frames.len() as u32 {
            if self.state[cell as usize] != 0 {
                self.set_state(cell, 255);
            }
            self.mark(cell);
        }
        self.detector.invalidate();
        true
    }

    pub fn set_edges_only(&mut self, edges_only: bool) {
        self.edges_only = edges_only;
        self.counts.iter_mut().for_each(|c| *c = 0);
        for c in 0..self.frames.len() {
            if self.state[c] != 0 {
                for i in 0..self.neighbour_range(c) {
                    let n = self.neighbours[c][i] as usize;
                    self.counts[n] += 1;
                }
            }
        }
        for cell in 0..self.frames.len() as u32 {
            self.mark(cell);
        }
        self.detector.invalidate();
    }

    pub fn clear(&mut self) {
        for cell in 0..self.frames.len() as u32 {
            if self.state[cell as usize] != 0 {
                self.flip(cell);
            }
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
        for cell in 0..self.frames.len() as u32 {
            let at = self.frames[cell as usize].at.lattice();
            if (at.x() - ax).hypot(at.y() - ay) > radius {
                continue;
            }
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

        let mut detector = std::mem::take(&mut self.detector);
        detector.observe(&*self);
        self.detector = detector;
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
