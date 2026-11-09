//! Life on the hat tiling, on the graph of `graph_world.rs`.

use crate::graph_world::{CellWorld, Outline, Tiling};
pub use crate::graph_world::{CellKey, MAX_RADIUS};
use crate::hat::{Frame, Hex, Hierarchy, LEVELS, Split, directed_edge, hat_points};

const ROOT_LEVEL: usize = LEVELS - 1;
const ROOT_KIND: usize = 0;
/// Pieces are level 2 metatiles, which hold up to 169 hats.
const PIECE_LEVEL: usize = 2;

pub(crate) fn point_key(p: Hex) -> u64 {
    ((p.q as u32 as u64) << 32) | p.r as u32 as u64
}

/// Lattice coordinates stay well within 26 bits and edges are short, so one word
/// holds an edge: start point, then the offset to its end point.
fn edge_key(from: Hex, to: Hex) -> u64 {
    let (dq, dr) = ((to.q - from.q + 4) as u64, (to.r - from.r + 4) as u64);
    (((from.q + (1 << 25)) as u64) << 38) | (((from.r + (1 << 25)) as u64) << 12) | (dq << 6) | dr
}

fn center_of(frame: &Frame, p: (f64, f64)) -> (f64, f64) {
    let y = if frame.flip { -p.1 } else { p.1 };
    let (s, c) = (frame.rotation as f64 * std::f64::consts::PI / 3.0).sin_cos();
    let at = frame.at.lattice();
    (c * p.0 - s * y + at.x(), s * p.0 + c * y + at.y())
}

/// A hat and the H, T, P or F metatile around it, see `metatile`.
#[derive(Clone, Copy)]
pub struct HatCell {
    frame: Frame,
    metatile: u8,
}

impl PartialEq for HatCell {
    fn eq(&self, other: &Self) -> bool {
        self.frame == other.frame
    }
}

impl std::ops::Deref for HatCell {
    type Target = Frame;
    fn deref(&self) -> &Frame {
        &self.frame
    }
}

pub struct HatTiling {
    hierarchy: Hierarchy,
    root: Frame,
    /// Split position of the hat nearest the origin, subtracted from all output.
    origin: Split,
    /// Centre of a hat relative to its position, by `rotation + 8 * flip`.
    center_offsets: [(f64, f64); 16],
    ratio: (f64, f64),
}

impl Tiling for HatTiling {
    type Cell = HatCell;
    type Piece = (usize, Frame);
    type Point = u64;
    type Edge = u64;

    const MAX_NEIGHBOURS: usize = 10;
    /// More than the longest diameter of a hat, in lattice units.
    const RING_MARGIN: f64 = 8.0;

    fn pieces_near(&self, x: f64, y: f64, radius: f64, out: &mut Vec<Self::Piece>) {
        let mut stack = vec![(ROOT_LEVEL, ROOT_KIND, self.root)];
        while let Some((level, kind, frame)) = stack.pop() {
            let proto = &self.hierarchy.levels[level][kind];
            let c = center_of(&frame, proto.center);
            if (c.0 - x).hypot(c.1 - y) > radius + proto.radius {
                continue;
            }
            if level == PIECE_LEVEL {
                out.push((kind, frame));
                continue;
            }
            for child in &proto.children {
                stack.push((level - 1, child.kind, frame.then(&child.frame)));
            }
        }
    }

    fn bounds(&self, &(kind, frame): &Self::Piece) -> ((f64, f64), f64) {
        let proto = &self.hierarchy.levels[PIECE_LEVEL][kind];
        (center_of(&frame, proto.center), proto.radius)
    }

    fn cells_of(&mut self, &(kind, frame): &Self::Piece, out: &mut Vec<HatCell>) {
        crate::hat::for_each_hat_tagged(&self.hierarchy, PIECE_LEVEL, kind, &frame, &mut |f, metatile| {
            out.push(HatCell { frame: *f, metatile: metatile as u8 });
        });
    }

    fn outline(&self, cell: &HatCell, out: &mut Outline<Self>) {
        let points = hat_points(&self.hierarchy, &cell.frame);
        for k in 0..14 {
            let (from, to) = directed_edge(&cell.frame, &points, k);
            out.edges.push((edge_key(from, to), edge_key(to, from)));
        }
        out.points.extend(points.iter().map(|p| point_key(*p)));
    }

    fn origin(&self) -> (f64, f64) {
        let o = self.origin.lattice();
        (o.x(), o.y())
    }

    fn place(&self, cell: &HatCell) -> (f64, f64) {
        let at = cell.frame.at.lattice();
        (at.x(), at.y())
    }

    fn center(&self, cell: &HatCell) -> (f64, f64) {
        let frame = &cell.frame;
        let at = frame.at.lattice();
        let origin = self.origin.lattice();
        let offset = self.center_offsets[frame.rotation as usize + if frame.flip { 8 } else { 0 }];
        ((at.x() - origin.x()) + offset.0, (at.y() - origin.y()) + offset.1)
    }

    fn order(&self, cell: &HatCell) -> [i64; 6] {
        let f = &cell.frame;
        [f.at.p.q, f.at.p.r, f.at.q.q, f.at.q.r, f.rotation as i64, f.flip as i64]
    }

    fn key(&self, cell: &HatCell) -> CellKey {
        let frame = &cell.frame;
        [
            frame.rotation as i32 + if frame.flip { 8 } else { 0 },
            (frame.at.p.q - self.origin.p.q) as i32,
            (frame.at.p.r - self.origin.p.r) as i32,
            (frame.at.q.q - self.origin.q.q) as i32,
            (frame.at.q.r - self.origin.q.r) as i32,
        ]
    }

    fn from_key(&self, key: CellKey) -> Option<(HatCell, u64, (f64, f64))> {
        let rotation = key[0] & 7;
        if !(0..16).contains(&key[0]) || rotation >= 6 {
            return None;
        }
        let frame = Frame {
            rotation: rotation as u8,
            flip: key[0] & 8 != 0,
            at: Split {
                p: Hex::new(key[1] as i64 + self.origin.p.q, key[2] as i64 + self.origin.p.r),
                q: Hex::new(key[3] as i64 + self.origin.q.q, key[4] as i64 + self.origin.q.r),
            },
        };
        let at = frame.at.lattice();
        Some((HatCell { frame, metatile: 0 }, point_key(at), (at.x(), at.y())))
    }
}

pub type HatWorld = CellWorld<HatTiling>;

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

        let mut world = CellWorld::with_tiling(HatTiling { hierarchy, root, origin: Split::default(), center_offsets, ratio });

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
        world.tiling.origin = nearest.at;
        world
    }

    pub fn ratio(&self) -> (f64, f64) {
        self.tiling.ratio
    }

    /// 0 to 3 for H, T, P and F.
    pub fn metatile(&self, cell: u32) -> u8 {
        self.frames[cell as usize].metatile
    }

    /// A mirrored hat. Each H metatile holds one, and nothing else does.
    pub fn is_mirrored(&self, cell: u32) -> bool {
        self.frames[cell as usize].flip
    }

    pub fn orientation(&self, cell: u32) -> u8 {
        self.frames[cell as usize].rotation
    }

    pub fn export_outline(&mut self) -> &[i32] {
        self.export.clear();
        for v in &self.tiling.hierarchy.outline {
            self.export.extend([v.p.q as i32, v.p.r as i32, v.q.q as i32, v.q.r as i32]);
        }
        &self.export
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::Status;
    use std::collections::{HashMap, HashSet as Set};

    fn live(w: &HatWorld) -> Vec<u32> {
        (0..w.frames.len() as u32).filter(|&c| w.state[c as usize] != 0).collect()
    }

    /// Recomputes the next generation from scratch, ignoring all bookkeeping.
    fn naive_next(w: &HatWorld, birth: u16, survive: u16) -> Set<u32> {
        let mut next = Set::new();
        for c in 0..w.frames.len() {
            let range = w.neighbour_range(c);
            let n = w.row(c)[..range].iter().filter(|&&o| w.state[o as usize] != 0).count();
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
            let points = hat_points(&w.tiling.hierarchy, f);
            for k in 0..14 {
                by_point.entry(points[k]).or_default().push(i as u32);
                by_edge.insert(directed_edge(f, &points, k), i as u32);
            }
        }
        for (i, f) in w.frames.iter().enumerate() {
            let points = hat_points(&w.tiling.hierarchy, f);
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
            let all: Set<u32> = w.row(i)[..w.all_neighbours[i] as usize].iter().copied().collect();
            let front: Set<u32> = w.row(i)[..w.edge_neighbours[i] as usize].iter().copied().collect();
            assert_eq!(all, vertex, "cell {i}");
            assert_eq!(front, edge, "cell {i}");
            for o in &all {
                assert!(w.row(*o as usize).contains(&(i as u32)));
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
        let start = w.cell_count();
        w.load_around(500_000.0, -300_000.0, 40.0);
        assert!(w.cell_count() > start);
        // The far patch has its own hats, whose splits must fit the ratio estimate.
        let (re, im) = w.ratio();
        let far = w.frames[w.frames.len() - 1].at;
        let (px, py, qx, qy) = (
            (far.p.q - w.tiling.origin.p.q) as f64 + 0.5 * (far.p.r - w.tiling.origin.p.r) as f64,
            0.866_025_403_784_438_6 * (far.p.r - w.tiling.origin.p.r) as f64,
            (far.q.q - w.tiling.origin.q.q) as f64 + 0.5 * (far.q.r - w.tiling.origin.q.r) as f64,
            0.866_025_403_784_438_6 * (far.q.r - w.tiling.origin.q.r) as f64,
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
