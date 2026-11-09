//! Penrose's rhomb tiling (P3), on the graph of `graph_world.rs`.
//!
//! Generated top down by subdividing Robinson triangles, starting from a wheel of
//! ten: each rhomb is two triangles mirrored across its diagonal. Positions are exact.
//! All edges point along tenth roots of unity, so every vertex is a whole-numbered
//! combination of 1, z, z^2 and z^3 with z = e^(2 pi i / 5), and dividing by the golden
//! ratio, which subdividing needs, stays within those numbers.

use crate::graph_world::{CellKey, CellWorld, Outline, Tiling};
use std::collections::HashSet;
use std::f64::consts::PI;

/// Subdivisions from the wheel down to rhombs of edge length 1. The wheel's radius is
/// the golden ratio to this power, about 13 million.
const DEPTH: u32 = 34;
/// Subdivisions from a piece down to rhombs, which makes some two dozen per piece.
const PIECE_DEPTH: u32 = 4;
const FIELD_RADIUS: f64 = 12_000_000.0;

type Z = [i32; 4];

fn add(a: Z, b: Z) -> Z {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}

fn sub(a: Z, b: Z) -> Z {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
}

/// Multiplies by z, using z^4 = -1 - z - z^2 - z^3.
fn times_z(a: Z) -> Z {
    [-a[3], a[0] - a[3], a[1] - a[3], a[2] - a[3]]
}

/// Divides by the golden ratio, which is the same as multiplying by z + z^4. Not
/// inlined: unrolled in the loop of `PenroseTiling::new`, this keeps the optimiser
/// busy for many minutes.
#[inline(never)]
fn over_phi(a: Z) -> Z {
    let once = times_z(a);
    let four = times_z(times_z(times_z(once)));
    add(once, four)
}

fn times_phi(a: Z) -> Z {
    add(a, over_phi(a))
}

/// The tenth roots of unity: e^(i pi / 5) is -z^3, and z^5 = 1.
const UNITS: [Z; 10] = [
    [1, 0, 0, 0],
    [0, 0, 0, -1],
    [0, 1, 0, 0],
    [1, 1, 1, 1],
    [0, 0, 1, 0],
    [-1, 0, 0, 0],
    [0, 0, 0, 1],
    [0, -1, 0, 0],
    [-1, -1, -1, -1],
    [0, 0, -1, 0],
];

fn unit(k: usize) -> Z {
    UNITS[k % 10]
}

fn direction_of(v: Z) -> usize {
    UNITS.iter().position(|u| *u == v).expect("rhomb edges have length 1")
}

/// The lattice of vertices is dense in the plane, but vertices of one tiling lie in a
/// bounded window of the other embedding of these numbers. So two of them with all four
/// parts equal modulo 2^16 would have to be further apart than the field is wide (the
/// product of both embeddings' norms is a whole multiple of 2^64), and 16 bits per part
/// identify a vertex. A test checks that.
fn point_key(p: Z) -> u64 {
    (p[0] as u16 as u64) << 48 | (p[1] as u16 as u64) << 32 | (p[2] as u16 as u64) << 16 | p[3] as u16 as u64
}

fn position(p: Z) -> (f64, f64) {
    let (mut x, mut y) = (0.0, 0.0);
    for (i, &part) in p.iter().enumerate() {
        let (s, c) = (2.0 * PI * i as f64 / 5.0).sin_cos();
        x += part as f64 * c;
        y += part as f64 * s;
    }
    (x, y)
}

/// A Robinson triangle. `a` is its tip, and `b` to `c` the diagonal of its rhomb.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct Triangle {
    thick: bool,
    a: Z,
    b: Z,
    c: Z,
}

impl Triangle {
    fn children(&self, out: &mut Vec<Triangle>) {
        let Triangle { thick, a, b, c } = *self;
        if thick {
            let q = add(b, over_phi(sub(a, b)));
            let r = add(b, over_phi(sub(c, b)));
            out.push(Triangle { thick: true, a: r, b: c, c: a });
            out.push(Triangle { thick: true, a: q, b: r, c: b });
            out.push(Triangle { thick: false, a: r, b: q, c: a });
        } else {
            let p = add(a, over_phi(sub(b, a)));
            out.push(Triangle { thick: false, a: c, b: p, c: b });
            out.push(Triangle { thick: true, a: p, b: c, c: a });
        }
    }

    fn bounds(&self) -> ((f64, f64), f64) {
        let (pa, pb, pc) = (position(self.a), position(self.b), position(self.c));
        let center = ((pa.0 + pb.0 + pc.0) / 3.0, (pa.1 + pb.1 + pc.1) / 3.0);
        let reach = |p: (f64, f64)| (p.0 - center.0).hypot(p.1 - center.1);
        (center, reach(pa).max(reach(pb)).max(reach(pc)))
    }
}

/// A rhomb: starts at `anchor`, with edges along `unit(turn)` and then, counterclockwise,
/// along `unit(turn + 1)` if thin or `unit(turn + 3)` if thick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rhomb {
    anchor: Z,
    turn: u8,
    thick: bool,
}

impl Rhomb {
    fn spread(&self) -> usize {
        if self.thick { 3 } else { 1 }
    }

    /// Two opposite corners could be the anchor, so one is chosen.
    fn from_triangle(t: &Triangle) -> Rhomb {
        let spread = if t.thick { 3 } else { 1 };
        let (toward_b, toward_c) = (direction_of(sub(t.b, t.a)), direction_of(sub(t.c, t.a)));
        let turn = if (toward_b + spread) % 10 == toward_c { toward_b } else { toward_c };
        let far = sub(add(t.b, t.c), t.a);
        let (anchor, turn) = if t.a <= far { (t.a, turn) } else { (far, (turn + 5) % 10) };
        Rhomb { anchor, turn: turn as u8, thick: t.thick }
    }

    fn corners(&self) -> [Z; 4] {
        let turn = self.turn as usize;
        let (u, v) = (unit(turn), unit(turn + self.spread()));
        [self.anchor, add(self.anchor, u), add(add(self.anchor, u), v), add(self.anchor, v)]
    }

    fn code(&self) -> i32 {
        self.turn as i32 + if self.thick { 16 } else { 0 }
    }
}

pub struct PenroseTiling {
    wheel: Vec<Triangle>,
    /// A rhomb can span two pieces, and only the first one brings it.
    claimed: HashSet<(u64, u8), crate::graph_world::Fast>,
}

impl PenroseTiling {
    fn new() -> Self {
        let mut wheel = Vec::new();
        for i in 0..10 {
            let scale = |mut v: Z| {
                for _ in 0..DEPTH {
                    v = times_phi(v);
                }
                v
            };
            let (first, second) = (scale(unit(i)), scale(unit(i + 1)));
            // Neighbours are mirror images, so that they share a whole edge.
            let (b, c) = if i % 2 == 0 { (second, first) } else { (first, second) };
            wheel.push(Triangle { thick: false, a: [0; 4], b, c });
        }
        PenroseTiling { wheel, claimed: HashSet::default() }
    }
}

impl Tiling for PenroseTiling {
    type Cell = Rhomb;
    type Piece = Triangle;
    type Point = u64;
    /// A start point and the direction of the edge.
    type Edge = (u64, u8);

    /// Each of 4 corners has at most 7 tiles, the rhomb among them, and the two tiles
    /// next to it at a corner are shared with the edges: 4 + 4 * (7 - 3).
    const MAX_NEIGHBOURS: usize = 20;
    /// Two long diagonals of a thick rhomb, and some room.
    const RING_MARGIN: f64 = 4.0;

    fn pieces_near(&self, x: f64, y: f64, radius: f64, out: &mut Vec<Triangle>) {
        let mut stack: Vec<(u32, Triangle)> = self.wheel.iter().map(|t| (0, *t)).collect();
        let mut children = Vec::with_capacity(3);
        while let Some((depth, triangle)) = stack.pop() {
            let (center, reach) = triangle.bounds();
            if (center.0 - x).hypot(center.1 - y) > radius + reach {
                continue;
            }
            if depth == DEPTH - PIECE_DEPTH {
                out.push(triangle);
                continue;
            }
            children.clear();
            triangle.children(&mut children);
            stack.extend(children.iter().map(|t| (depth + 1, *t)));
        }
    }

    fn bounds(&self, piece: &Triangle) -> ((f64, f64), f64) {
        piece.bounds()
    }

    fn cells_of(&mut self, piece: &Triangle, out: &mut Vec<Rhomb>) {
        let mut level = vec![*piece];
        let mut next = Vec::new();
        for _ in 0..PIECE_DEPTH {
            next.clear();
            level.iter().for_each(|t| t.children(&mut next));
            std::mem::swap(&mut level, &mut next);
        }
        for triangle in &level {
            let rhomb = Rhomb::from_triangle(triangle);
            // Far out at the wheel's rim, the rhomb's other half does not exist.
            let far = position(rhomb.corners()[2]);
            if far.0.hypot(far.1) < FIELD_RADIUS && self.claimed.insert((point_key(rhomb.anchor), rhomb.code() as u8)) {
                out.push(rhomb);
            }
        }
    }

    fn outline(&self, cell: &Rhomb, out: &mut Outline<Self>) {
        let corners = cell.corners();
        let turn = cell.turn as usize;
        let spread = cell.spread();
        let directions = [turn, turn + spread, turn + 5, turn + spread + 5];
        for i in 0..4 {
            let (from, to) = (point_key(corners[i]), point_key(corners[(i + 1) % 4]));
            out.edges.push(((from, (directions[i] % 10) as u8), (to, ((directions[i] + 5) % 10) as u8)));
        }
        out.points.extend(corners.iter().map(|p| point_key(*p)));
    }

    fn origin(&self) -> (f64, f64) {
        (0.0, 0.0)
    }

    fn place(&self, cell: &Rhomb) -> (f64, f64) {
        self.center(cell)
    }

    fn center(&self, cell: &Rhomb) -> (f64, f64) {
        let (a, b) = (position(cell.corners()[0]), position(cell.corners()[2]));
        ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
    }

    fn order(&self, cell: &Rhomb) -> [i64; 6] {
        let a = cell.anchor;
        [a[0] as i64, a[1] as i64, a[2] as i64, a[3] as i64, cell.turn as i64, cell.thick as i64]
    }

    fn key(&self, cell: &Rhomb) -> CellKey {
        [cell.code(), cell.anchor[0], cell.anchor[1], cell.anchor[2], cell.anchor[3]]
    }

    fn from_key(&self, key: CellKey) -> Option<(Rhomb, u64, (f64, f64))> {
        let turn = key[0] & 15;
        if !(0..32).contains(&key[0]) || turn >= 10 {
            return None;
        }
        let anchor = [key[1], key[2], key[3], key[4]];
        // Beyond this, the arithmetic of `point_key` makes no promises.
        if anchor.iter().any(|part| part.abs() > 1 << 28) {
            return None;
        }
        let at = position(anchor);
        if at.0.hypot(at.1) > FIELD_RADIUS {
            return None;
        }
        Some((Rhomb { anchor, turn: turn as u8, thick: key[0] & 16 != 0 }, point_key(anchor), at))
    }
}

pub type PenroseWorld = CellWorld<PenroseTiling>;

impl Default for PenroseWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl PenroseWorld {
    pub fn new() -> Self {
        let mut world = CellWorld::with_tiling(PenroseTiling::new());
        world.load_absolute(0.0, 0.0, 12.0);
        world
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet as Set};

    const PHI: f64 = 1.618_033_988_749_895;

    fn area(r: &Rhomb) -> f64 {
        if r.thick { (2.0 * PI / 5.0).sin() } else { (PI / 5.0).sin() }
    }

    fn inner_cells(w: &PenroseWorld, within: f64) -> Vec<usize> {
        (0..w.frames.len()).filter(|&c| { let p = w.center(c as u32); p.0.hypot(p.1) < within }).collect()
    }

    #[test]
    fn arithmetic_matches_the_numbers_it_stands_for() {
        // Each unit is the one before it times -z^3.
        for k in 0..10 {
            let thrice = times_z(times_z(times_z(unit(k))));
            assert_eq!(unit(k + 1), [-thrice[0], -thrice[1], -thrice[2], -thrice[3]]);
            let (x, y) = position(unit(k));
            let angle = k as f64 * PI / 5.0;
            assert!((x - angle.cos()).abs() < 1e-12 && (y - angle.sin()).abs() < 1e-12, "unit {k}");
        }
        let v = [3, -7, 11, 2];
        let (x, y) = position(v);
        let (sx, sy) = position(over_phi(v));
        assert!((sx * PHI - x).abs() < 1e-9 && (sy * PHI - y).abs() < 1e-9);
        assert_eq!(times_phi(over_phi(v)), v);
    }

    #[test]
    fn rhombs_are_real_with_edges_of_length_one() {
        let mut w = PenroseWorld::new();
        w.load_around(0.0, 0.0, 40.0);
        for r in &w.frames {
            let corners = r.corners();
            let at: Vec<(f64, f64)> = corners.iter().map(|p| position(*p)).collect();
            for i in 0..4 {
                let (a, b) = (at[i], at[(i + 1) % 4]);
                assert!(((a.0 - b.0).hypot(a.1 - b.1) - 1.0).abs() < 1e-9);
            }
            // Counterclockwise, and of the area the kind of rhomb has.
            let shoelace: f64 = (0..4).map(|i| at[i].0 * at[(i + 1) % 4].1 - at[(i + 1) % 4].0 * at[i].1).sum();
            assert!((shoelace / 2.0 - area(r)).abs() < 1e-9, "{r:?}");
        }
    }

    /// Known properties of Penrose's rhomb tilings, none of which the construction
    /// asks for.
    #[test]
    fn it_is_a_penrose_tiling() {
        let mut w = PenroseWorld::new();
        w.load_around(30.0, -20.0, 130.0);

        // Everything fills the disc with no gap or overlap: the areas add up.
        let (cx, cy) = (30.0, -20.0);
        let radius = 100.0f64;
        let filled: f64 = w.frames.iter().filter(|r| {
            let p = w.tiling.center(r);
            (p.0 - cx).hypot(p.1 - cy) < radius
        }).map(area).sum();
        let disc = PI * radius * radius;
        assert!((filled / disc - 1.0).abs() < 0.01, "areas {filled} of {disc}");

        let inner = inner_cells(&w, 90.0);
        let mut degrees: HashMap<u64, Set<u32>> = HashMap::new();
        for (i, r) in w.frames.iter().enumerate() {
            for p in r.corners() {
                degrees.entry(point_key(p)).or_default().insert(i as u32);
            }
        }
        for &c in &inner {
            assert_eq!(w.edge_neighbours[c], 4, "cell {c} is not surrounded");
            for p in w.frames[c].corners() {
                let degree = degrees[&point_key(p)].len();
                assert!((3..=7).contains(&degree), "degree {degree}");
            }
        }

        // Thick rhombs outnumber thin ones by the golden ratio.
        let thick = inner.iter().filter(|&&c| w.frames[c].thick).count() as f64;
        let thin = inner.len() as f64 - thick;
        assert!((thick / thin - PHI).abs() < 0.02, "ratio {}", thick / thin);
        let most = inner.iter().map(|&c| w.all_neighbours[c]).max().unwrap();
        assert!(most as usize <= PenroseTiling::MAX_NEIGHBOURS);
    }

    #[test]
    fn neighbours_match_the_geometry() {
        let mut w = PenroseWorld::new();
        w.load_around(0.0, 0.0, 60.0);
        let mut by_point: HashMap<Z, Vec<u32>> = HashMap::new();
        for (i, r) in w.frames.iter().enumerate() {
            for p in r.corners() {
                by_point.entry(p).or_default().push(i as u32);
            }
        }
        for c in inner_cells(&w, 45.0) {
            let corners = w.frames[c].corners();
            let mut touching: Set<u32> = corners.iter().flat_map(|p| by_point[p].iter().copied()).collect();
            touching.remove(&(c as u32));
            let all: Set<u32> = w.row(c)[..w.all_neighbours[c] as usize].iter().copied().collect();
            assert_eq!(all, touching, "cell {c}");
            // A neighbour over an edge shares two corners next to each other.
            for &n in &w.row(c)[..w.edge_neighbours[c] as usize] {
                let shared = w.frames[n as usize].corners().iter().filter(|p| corners.contains(p)).count();
                assert_eq!(shared, 2, "cell {c} and {n}");
            }
            for &n in &all {
                assert!(w.row(n as usize).contains(&(c as u32)));
            }
        }
    }

    #[test]
    fn vertex_keys_tell_vertices_apart() {
        for (x, y) in [(0.0, 0.0), (9_000_000.0, 7_000_000.0), (-11_000_000.0, 300.0)] {
            let mut w = PenroseWorld::new();
            w.load_around(x, y, 80.0);
            let mut seen: HashMap<u64, Z> = HashMap::new();
            for r in &w.frames {
                for p in r.corners() {
                    assert_eq!(*seen.entry(point_key(p)).or_insert(p), p);
                }
            }
        }
    }

    #[test]
    fn keys_mean_the_same_in_any_world() {
        let mut first = PenroseWorld::new();
        first.load_around(500.0, -300.0, 20.0);
        let cells: Vec<u32> = (0..first.frames.len() as u32).step_by(13).collect();
        let mut other = PenroseWorld::new();
        for &cell in &cells {
            let key = first.key_of(cell);
            let there = other.cell_of(key).expect("the rhomb exists");
            assert_eq!(other.key_of(there), key);
            assert_eq!(other.center(there), first.center(cell));
        }
    }

    #[test]
    fn keys_of_no_rhomb_are_refused() {
        let mut w = PenroseWorld::new();
        assert_eq!(w.cell_of([10, 0, 0, 0, 0]), None);
        assert_eq!(w.cell_of([-1, 0, 0, 0, 0]), None);
        assert_eq!(w.cell_of([0, 7, 7, 7, 7]), None);
        assert_eq!(w.cell_of([0, i32::MAX, i32::MIN, i32::MAX, i32::MIN]), None);
    }

    fn naive_next(w: &PenroseWorld, birth: u16, survive: u16) -> Set<u32> {
        let mut next = Set::new();
        for c in 0..w.frames.len() {
            let n = w.row(c)[..w.neighbour_range(c)].iter().filter(|&&o| w.state[o as usize] != 0).count();
            let mask = if w.state[c] != 0 { survive } else { birth };
            if mask >> n & 1 == 1 {
                next.insert(c as u32);
            }
        }
        next
    }

    #[test]
    fn matches_a_full_recount_on_either_neighbourhood() {
        for (edges_only, birth, survive) in [(false, 1u16 << 3, 1u16 << 2 | 1 << 3), (true, 1 << 2, 1 << 1 | 1 << 2)] {
            let mut w = PenroseWorld::new();
            w.set_edges_only(edges_only);
            assert!(w.set_rule(birth, survive));
            w.soup(0.0, 0.0, 40.0, 0.35, 12345);
            for generation in 0..60 {
                let expected = naive_next(&w, birth, survive);
                w.step();
                let actual: Set<u32> = w.live().iter().copied().collect();
                assert_eq!(actual, expected, "generation {generation}");
                assert_eq!(w.population as usize, expected.len());
            }
        }
    }

    /// The field generated on demand must behave like one that was there all along.
    #[test]
    fn generating_on_demand_changes_nothing() {
        for edges_only in [false, true] {
            let mut lazy = PenroseWorld::new();
            let mut full = PenroseWorld::new();
            full.load_around(0.0, 0.0, 300.0);
            for w in [&mut lazy, &mut full] {
                w.set_edges_only(edges_only);
                assert!(w.set_rule(1 << 2 | 1 << 3, 1 << 2 | 1 << 3));
                w.soup(0.0, 0.0, 15.0, 0.4, 77);
            }
            for generation in 0..80 {
                let keys = |w: &PenroseWorld| -> Set<CellKey> { w.live().iter().map(|&c| w.key_of(c)).collect() };
                assert_eq!(keys(&lazy), keys(&full), "generation {generation}");
                lazy.step();
                full.step();
            }
        }
    }

    #[test]
    fn a_seed_means_the_same_field_wherever_the_view_has_been() {
        let keys = |w: &PenroseWorld| -> Set<CellKey> { w.live().iter().map(|&c| w.key_of(c)).collect() };
        let mut direct = PenroseWorld::new();
        direct.soup(10.0, -5.0, 30.0, 0.3, 4711);
        let mut wandered = PenroseWorld::new();
        wandered.load_around(90.0, 70.0, 30.0);
        wandered.soup(10.0, -5.0, 30.0, 0.3, 4711);
        assert!(direct.population > 0);
        assert_eq!(keys(&direct), keys(&wandered));
    }

    #[test]
    fn explosive_growth_stops_at_the_limit() {
        let mut w = PenroseWorld::new();
        w.set_max_cells(20_000);
        assert!(w.set_rule(1 << 1, (1 << 7) - 1));
        w.soup(0.0, 0.0, 15.0, 0.5, 3);
        while !w.capped {
            w.step();
            assert!(w.generation < 5000, "only {} cells so far", w.cell_count());
        }
        for _ in 0..3 {
            w.step();
        }
    }

    /// How many cells can touch one, which decides what the rule panel has to offer.
    #[test]
    fn neighbour_counts_fit_a_rule() {
        let mut w = PenroseWorld::new();
        w.load_around(0.0, 0.0, 400.0);
        let inner = inner_cells(&w, 380.0);
        let counts: Set<u8> = inner.iter().map(|&c| w.all_neighbours[c]).collect();
        let (low, high) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
        println!("edges and corners: {low} to {high}");
        assert!(*high <= 15, "a rule holds counts up to 15, found {high}");
    }
}
