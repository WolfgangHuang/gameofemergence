//! The hierarchy of the hat tiling, in exact integer coordinates.
//!
//! The substitution rules (`PATCH_RULES` and the construction in `construct`) are
//! taken from Craig S. Kaplan's hatviz, BSD 3-Clause, see public/THIRD_PARTY_NOTICES.txt.
//! They build each level of the H, T, P and F metatiles from the level below.
//!
//! Metatile outlines are only combinatorially meaningful and leave the lattice
//! after a few levels, while all hats stay on it. So the outlines are worked out
//! in floating point, and the positions of children are then re-expressed relative
//! to each metatile's first hat, which makes them exact.
//!
//! Every vertex has two integer parts, `p` from edges of length a and `q` from
//! edges of length b. The hat's own vertices are p + q; any other member of the
//! Tile(a, b) family is a * p + b / sqrt(3) * q, on the same combinatorics. Those
//! parts cannot be recovered from a position, so each metatile derives them for
//! its children at the point where two of them touch.

use std::collections::BinaryHeap;

const HR3: f64 = 0.866_025_403_784_438_6;
pub const LEVELS: usize = 16;
const HAT_CHILD: usize = usize::MAX;
const SEARCH_BUDGET: u32 = 400_000;

/// A number q + r * w, where w = e^(i pi / 3).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Hex {
    pub q: i64,
    pub r: i64,
}

impl Hex {
    pub const fn new(q: i64, r: i64) -> Self {
        Hex { q, r }
    }
    fn add(self, o: Hex) -> Hex {
        Hex::new(self.q + o.q, self.r + o.r)
    }
    fn sub(self, o: Hex) -> Hex {
        Hex::new(self.q - o.q, self.r - o.r)
    }
    fn rotated(self, steps: u8) -> Hex {
        let mut v = self;
        for _ in 0..steps % 6 {
            v = Hex::new(-v.r, v.q + v.r);
        }
        v
    }
    fn conjugate(self) -> Hex {
        Hex::new(self.q + self.r, -self.r)
    }
    pub fn x(self) -> f64 {
        self.q as f64 + 0.5 * self.r as f64
    }
    pub fn y(self) -> f64 {
        HR3 * self.r as f64
    }
}

/// A point split into the part along a-edges and the part along b-edges.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Split {
    pub p: Hex,
    pub q: Hex,
}

impl Split {
    pub fn lattice(self) -> Hex {
        self.p.add(self.q)
    }
    fn add(self, o: Split) -> Split {
        Split { p: self.p.add(o.p), q: self.q.add(o.q) }
    }
    fn sub(self, o: Split) -> Split {
        Split { p: self.p.sub(o.p), q: self.q.sub(o.q) }
    }
}

/// An isometry of the lattice: optional conjugation, then rotation by
/// `rotation * 60` degrees, then translation.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Frame {
    pub rotation: u8,
    pub flip: bool,
    pub at: Split,
}

impl Frame {
    fn linear(&self, v: Hex) -> Hex {
        let v = if self.flip { v.conjugate() } else { v };
        v.rotated(self.rotation)
    }
    fn linear_split(&self, v: Split) -> Split {
        Split { p: self.linear(v.p), q: self.linear(v.q) }
    }
    pub fn apply(&self, v: Split) -> Split {
        self.linear_split(v).add(self.at)
    }
    pub fn then(&self, inner: &Frame) -> Frame {
        Frame {
            rotation: if self.flip {
                (self.rotation + 6 - inner.rotation % 6) % 6
            } else {
                (self.rotation + inner.rotation) % 6
            },
            flip: self.flip != inner.flip,
            at: self.apply(inner.at),
        }
    }
}

/// The hat's vertices with each edge cut into pieces of the family's two lengths,
/// so that it has 14 edges. Each entry is that vertex's split position.
pub fn hat_outline() -> [Split; 14] {
    const A: [Hex; 8] = [
        Hex::new(1, -1), Hex::new(1, 0), Hex::new(1, 0), Hex::new(0, 1),
        Hex::new(-1, 1), Hex::new(-1, 0), Hex::new(0, -1), Hex::new(-1, 0),
    ];
    const B: [Hex; 6] = [
        Hex::new(-1, -1), Hex::new(2, -1), Hex::new(1, 1),
        Hex::new(-1, 2), Hex::new(-2, 1), Hex::new(1, -2),
    ];
    // Walking the hat's boundary from hexPt(0, 0): each step is an edge of either
    // kind. 0 picks the next a-edge, 1 the next b-edge.
    const KINDS: [u8; 14] = [1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1];
    let mut out = [Split::default(); 14];
    let (mut ia, mut ib) = (0, 0);
    let mut at = Split::default();
    for i in 0..14 {
        out[i] = at;
        if KINDS[i] == 0 {
            at.p = at.p.add(A[ia]);
            ia += 1;
        } else {
            at.q = at.q.add(B[ib]);
            ib += 1;
        }
    }
    debug_assert_eq!(at, Split::default());
    out
}

type Mat = [f64; 6];
type Pt = (f64, f64);

fn mul(a: Mat, b: Mat) -> Mat {
    [
        a[0] * b[0] + a[1] * b[3],
        a[0] * b[1] + a[1] * b[4],
        a[0] * b[2] + a[1] * b[5] + a[2],
        a[3] * b[0] + a[4] * b[3],
        a[3] * b[1] + a[4] * b[4],
        a[3] * b[2] + a[4] * b[5] + a[5],
    ]
}

fn inv(t: Mat) -> Mat {
    let det = t[0] * t[4] - t[1] * t[3];
    [
        t[4] / det,
        -t[1] / det,
        (t[1] * t[5] - t[2] * t[4]) / det,
        -t[3] / det,
        t[0] / det,
        (t[2] * t[3] - t[0] * t[5]) / det,
    ]
}

fn pt_at(m: Mat, p: Pt) -> Pt {
    (m[0] * p.0 + m[1] * p.1 + m[2], m[3] * p.0 + m[4] * p.1 + m[5])
}

fn match_seg(p: Pt, q: Pt) -> Mat {
    [q.0 - p.0, p.1 - q.1, p.0, q.1 - p.1, q.0 - p.0, p.1]
}

fn match_two(p1: Pt, q1: Pt, p2: Pt, q2: Pt) -> Mat {
    mul(match_seg(p2, q2), inv(match_seg(p1, q1)))
}

fn trans(x: f64, y: f64) -> Mat {
    [1.0, 0.0, x, 0.0, 1.0, y]
}

fn rot(angle: f64) -> Mat {
    let (s, c) = angle.sin_cos();
    [c, -s, 0.0, s, c, 0.0]
}

fn rot_about(p: Pt, angle: f64) -> Mat {
    mul(trans(p.0, p.1), mul(rot(angle), trans(-p.0, -p.1)))
}

fn intersect(p1: Pt, q1: Pt, p2: Pt, q2: Pt) -> Pt {
    let d = (q2.1 - p2.1) * (q1.0 - p1.0) - (q2.0 - p2.0) * (q1.1 - p1.1);
    let u = ((q2.0 - p2.0) * (p1.1 - p2.1) - (q2.1 - p2.1) * (p1.0 - p2.0)) / d;
    (p1.0 + u * (q1.0 - p1.0), p1.1 + u * (q1.1 - p1.1))
}

fn add(a: Pt, b: Pt) -> Pt {
    (a.0 + b.0, a.1 + b.1)
}

fn sub(a: Pt, b: Pt) -> Pt {
    (a.0 - b.0, a.1 - b.1)
}

const H: usize = 0;
const T: usize = 1;
const P: usize = 2;
const F: usize = 3;

/// A metatile as built by the substitution, before moving to exact coordinates.
#[derive(Clone)]
struct Raw {
    shape: Vec<Pt>,
    /// Transform and metatile kind of the level below; `HAT_CHILD` for hats.
    children: Vec<(Mat, usize)>,
}

fn hex_pt(q: f64, r: f64) -> Pt {
    (q + 0.5 * r, HR3 * r)
}

fn initial() -> [Raw; 4] {
    let hat = |q: f64, r: f64| hex_pt(q, r);
    let (h5, h7, h9, h11) = (hat(4.0, -2.0), hat(4.0, 0.0), hat(2.0, 2.0), hat(0.0, 2.0));

    let h_shape = vec![(0.0, 0.0), (4.0, 0.0), (4.5, HR3), (2.5, 5.0 * HR3), (1.5, 5.0 * HR3), (-0.5, HR3)];
    let h_children = vec![
        (match_two(h5, h7, h_shape[5], h_shape[0]), HAT_CHILD),
        (match_two(h9, h11, h_shape[1], h_shape[2]), HAT_CHILD),
        (match_two(h5, h7, h_shape[3], h_shape[4]), HAT_CHILD),
        (
            mul(trans(2.5, HR3), mul([-0.5, -HR3, 0.0, HR3, -0.5, 0.0], [0.5, 0.0, 0.0, 0.0, -0.5, 0.0])),
            HAT_CHILD,
        ),
    ];

    let t_shape = vec![(0.0, 0.0), (3.0, 0.0), (1.5, 3.0 * HR3)];
    let t_children = vec![([0.5, 0.0, 0.5, 0.0, 0.5, HR3], HAT_CHILD)];

    let pair = vec![
        ([0.5, 0.0, 1.5, 0.0, 0.5, HR3], HAT_CHILD),
        (
            mul(trans(0.0, 2.0 * HR3), mul([0.5, HR3, 0.0, -HR3, 0.5, 0.0], [0.5, 0.0, 0.0, 0.0, 0.5, 0.0])),
            HAT_CHILD,
        ),
    ];
    let p_shape = vec![(0.0, 0.0), (4.0, 0.0), (3.0, 2.0 * HR3), (-1.0, 2.0 * HR3)];
    let f_shape = vec![(0.0, 0.0), (3.0, 0.0), (3.5, HR3), (3.0, 2.0 * HR3), (-1.0, 2.0 * HR3)];

    [
        Raw { shape: h_shape, children: h_children },
        Raw { shape: t_shape, children: t_children },
        Raw { shape: p_shape, children: pair.clone() },
        Raw { shape: f_shape, children: pair },
    ]
}

/// (other child, its edge) or (child, edge, child, edge), then kind and own edge.
const PATCH_RULES: [(usize, usize, Option<(usize, usize)>, usize, usize); 28] = [
    (0, 0, None, P, 2),
    (1, 0, None, H, 2),
    (2, 0, None, P, 2),
    (3, 0, None, H, 2),
    (4, 4, None, P, 2),
    (0, 4, None, F, 3),
    (2, 4, None, F, 3),
    (4, 1, Some((3, 2)), F, 0),
    (8, 3, None, H, 0),
    (9, 2, None, P, 0),
    (10, 2, None, H, 0),
    (11, 4, None, P, 2),
    (12, 0, None, H, 2),
    (13, 0, None, F, 3),
    (14, 2, None, F, 1),
    (15, 3, None, H, 4),
    (8, 2, None, F, 1),
    (17, 3, None, H, 0),
    (18, 2, None, P, 0),
    (19, 2, None, H, 2),
    (20, 4, None, F, 3),
    (20, 0, None, P, 2),
    (22, 0, None, H, 2),
    (23, 4, None, F, 3),
    (23, 0, None, F, 3),
    (16, 0, None, P, 2),
    (9, 4, Some((0, 2)), T, 2),
    (4, 0, None, F, 3),
];

const IDENTITY: Mat = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

fn construct(below: &[Raw; 4]) -> [Raw; 4] {
    let mut patch: Vec<(Mat, usize)> = vec![(IDENTITY, H)];
    for &(c1, e1, other, kind, edge) in &PATCH_RULES {
        let poly = &below[patch[c1].1].shape;
        let (p, q) = match other {
            None => (
                pt_at(patch[c1].0, poly[(e1 + 1) % poly.len()]),
                pt_at(patch[c1].0, poly[e1]),
            ),
            Some((c2, e2)) => (
                pt_at(patch[c2].0, below[patch[c2].1].shape[e2]),
                pt_at(patch[c1].0, poly[e1]),
            ),
        };
        let new_poly = &below[kind].shape;
        patch.push((
            match_two(new_poly[edge], new_poly[(edge + 1) % new_poly.len()], p, q),
            kind,
        ));
    }

    let eval = |n: usize, i: usize| pt_at(patch[n].0, below[patch[n].1].shape[i]);
    let bps1 = eval(8, 2);
    let bps2 = eval(21, 2);
    let rbps = pt_at(rot_about(bps1, -2.0 * std::f64::consts::PI / 3.0), bps2);
    let p72 = eval(7, 2);
    let p252 = eval(25, 2);
    let llc = intersect(bps1, rbps, eval(6, 2), p72);

    let third = rot(-std::f64::consts::PI / 3.0);
    let mut w = sub(eval(6, 2), llc);
    let mut h_shape = vec![llc, bps1];
    w = pt_at(third, w);
    h_shape.push(add(h_shape[1], w));
    h_shape.push(eval(14, 2));
    w = pt_at(third, w);
    h_shape.push(sub(h_shape[3], w));
    h_shape.push(eval(6, 2));

    let pick = |indices: &[usize]| -> Vec<(Mat, usize)> { indices.iter().map(|&i| patch[i]).collect() };
    let new_h = Raw { children: pick(&[0, 9, 16, 27, 26, 6, 1, 8, 10, 15]), shape: h_shape.clone() };
    let new_p = Raw {
        shape: vec![p72, add(p72, sub(bps1, llc)), bps1, llc],
        children: pick(&[7, 2, 3, 4, 28]),
    };
    let new_f = Raw {
        shape: vec![bps2, eval(24, 2), eval(25, 0), p252, add(p252, sub(llc, bps1))],
        children: pick(&[21, 20, 22, 23, 24, 25]),
    };
    let aaa = h_shape[2];
    let bbb = add(h_shape[1], sub(h_shape[4], h_shape[5]));
    let ccc = pt_at(rot_about(bbb, -std::f64::consts::PI / 3.0), aaa);
    let new_t = Raw { shape: vec![bbb, ccc, aaa], children: pick(&[11]) };

    [new_h, new_t, new_p, new_f]
}

#[derive(Clone, Debug)]
pub struct Child {
    /// Kind of the metatile one level below; meaningless when `Proto::leaf` is set.
    pub kind: usize,
    pub frame: Frame,
}

#[derive(Clone, Debug)]
pub struct Proto {
    pub leaf: bool,
    pub children: Vec<Child>,
    /// Bounds on the lattice, relative to this metatile's first hat.
    pub center: (f64, f64),
    pub radius: f64,
    /// Number of hats inside.
    pub hats: f64,
}

pub struct Hierarchy {
    /// `levels[0]` holds the metatiles made of hats.
    pub levels: Vec<[Proto; 4]>,
    pub outline: [Split; 14],
    /// Largest deviation from the lattice seen while snapping positions.
    pub worst_snap: f64,
}

fn snap_linear(m: &[f64; 4]) -> (u8, bool) {
    let det = m[0] * m[3] - m[1] * m[2];
    let flip = det < 0.0;
    let angle = m[2].atan2(m[0]);
    let steps = (angle / (std::f64::consts::PI / 3.0)).round() as i32;
    (steps.rem_euclid(6) as u8, flip)
}

fn linear_at(m: &[f64; 4], p: Pt) -> Pt {
    (m[0] * p.0 + m[1] * p.1, m[2] * p.0 + m[3] * p.1)
}

fn hex_of(p: Pt) -> (Hex, f64) {
    let r = p.1 / HR3;
    let q = p.0 - 0.5 * r;
    let (rq, rr) = (q.round(), r.round());
    (Hex::new(rq as i64, rr as i64), (q - rq).abs().max((r - rr).abs()))
}

#[derive(Clone, Copy)]
struct Node {
    level: i32,
    kind: usize,
    /// Position inside the child whose split is being derived, or inside the
    /// metatile being built for a child already known.
    frame: Frame,
    /// Lattice position in the metatile being built.
    placed: Frame,
}

struct Candidate {
    distance: f64,
    a: Node,
    b: Node,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.distance == other.distance
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Candidate {
    /// Reversed, so that the heap pops the closest pair first.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.distance.total_cmp(&self.distance)
    }
}

impl Hierarchy {
    pub fn new() -> Self {
        let outline = hat_outline();
        let mut raws = initial();
        let mut levels: Vec<[Proto; 4]> = Vec::with_capacity(LEVELS);
        // Offset of each metatile's first hat in its raw coordinates, in lattice units.
        let mut origins: Vec<[Pt; 4]> = Vec::with_capacity(LEVELS);
        let mut worst_snap: f64 = 0.0;

        for level in 0..LEVELS {
            let mut level_origins = [(0.0, 0.0); 4];
            let mut protos: Vec<Proto> = Vec::with_capacity(4);
            for kind in 0..4 {
                let raw = &raws[kind];
                let leaf = level == 0;
                // Hats sit at half scale in the outlines' coordinates, so
                // everything doubles to reach lattice units.
                let linear_of = |m: &Mat| -> [f64; 4] {
                    if leaf {
                        [m[0] * 2.0, m[1] * 2.0, m[3] * 2.0, m[4] * 2.0]
                    } else {
                        [m[0], m[1], m[3], m[4]]
                    }
                };
                let place = |m: &Mat, child: usize| -> Pt {
                    let l = linear_of(m);
                    let inner = if leaf { (0.0, 0.0) } else { origins[level - 1][child] };
                    add((2.0 * m[2], 2.0 * m[5]), linear_at(&l, inner))
                };
                let origin = place(&raw.children[0].0, raw.children[0].1);
                level_origins[kind] = origin;

                let mut children = Vec::with_capacity(raw.children.len());
                for (m, child) in &raw.children {
                    let l = linear_of(m);
                    let (rotation, flip) = snap_linear(&l);
                    let (at, deviation) = hex_of(sub(place(m, *child), origin));
                    worst_snap = worst_snap.max(deviation);
                    children.push(Child {
                        kind: if leaf { HAT_CHILD } else { *child },
                        // The split part comes later; for now all of it is in `p`.
                        frame: Frame { rotation, flip, at: Split { p: at, q: Hex::default() } },
                    });
                }
                protos.push(Proto { leaf, children, center: (0.0, 0.0), radius: 0.0, hats: 0.0 });
            }
            origins.push(level_origins);
            let protos: [Proto; 4] = protos.try_into().expect("four kinds");
            levels.push(protos);
            if level + 1 < LEVELS {
                raws = construct(&raws);
            }
        }

        let mut hierarchy = Hierarchy { levels, outline, worst_snap };
        for level in 0..LEVELS {
            hierarchy.finish_level(level);
        }
        hierarchy
    }

    fn hat_bounds(&self) -> ((f64, f64), f64) {
        let n = self.outline.len() as f64;
        let (mut cx, mut cy) = (0.0, 0.0);
        for v in &self.outline {
            cx += v.lattice().x() / n;
            cy += v.lattice().y() / n;
        }
        let radius = self
            .outline
            .iter()
            .map(|v| (v.lattice().x() - cx).hypot(v.lattice().y() - cy))
            .fold(0.0, f64::max);
        ((cx, cy), radius)
    }

    /// Bounds of a node, whose frame need not know its splits.
    fn node_bounds(&self, level: i32, kind: usize, frame: &Frame) -> ((f64, f64), f64) {
        let (center, radius) = if level < 0 {
            self.hat_bounds()
        } else {
            let proto = &self.levels[level as usize][kind];
            (proto.center, proto.radius)
        };
        (frame_point(frame, center), radius)
    }

    /// Computes bounds and hat counts, then derives the split parts of children.
    fn finish_level(&mut self, level: usize) {
        for kind in 0..4 {
            let (mut sum_x, mut sum_y, mut weight) = (0.0, 0.0, 0.0);
            let mut hats = 0.0;
            let children = self.levels[level][kind].children.clone();
            for child in &children {
                let child_level = level as i32 - 1;
                let (center, _) = self.node_bounds(child_level, child.kind, &child.frame);
                let count = if level == 0 { 1.0 } else { self.levels[level - 1][child.kind].hats };
                sum_x += center.0 * count;
                sum_y += center.1 * count;
                weight += count;
                hats += count;
            }
            let center = (sum_x / weight, sum_y / weight);
            let radius = children
                .iter()
                .map(|child| {
                    let (c, r) = self.node_bounds(level as i32 - 1, child.kind, &child.frame);
                    (c.0 - center.0).hypot(c.1 - center.1) + r
                })
                .fold(0.0, f64::max);
            let proto = &mut self.levels[level][kind];
            proto.center = center;
            proto.radius = radius;
            proto.hats = hats;
        }
        for kind in 0..4 {
            self.derive_splits(level, kind);
        }
    }

    /// Fixes the first child at the origin and walks outwards across touching
    /// children, each shared vertex telling the next child's split position.
    fn derive_splits(&mut self, level: usize, kind: usize) {
        let mut children = self.levels[level][kind].children.clone();
        let n = children.len();
        let mut known = vec![false; n];
        known[0] = true;
        // A child's lattice position is p + q, so the first one has q = 0 and
        // keeps its position in p; that is merely the choice of origin.
        let child_level = level as i32 - 1;

        let mut progress = true;
        while progress {
            progress = false;
            for j in 0..n {
                if known[j] {
                    continue;
                }
                let mut order: Vec<(f64, usize)> = (0..n)
                    .filter(|&i| known[i])
                    .map(|i| {
                        let (a, _) = self.node_bounds(child_level, children[i].kind, &children[i].frame);
                        let (b, _) = self.node_bounds(child_level, children[j].kind, &children[j].frame);
                        ((a.0 - b.0).hypot(a.1 - b.1), i)
                    })
                    .collect();
                order.sort_by(|x, y| x.0.total_cmp(&y.0));
                for (_, i) in order {
                    if let Some(at) = self.touching(child_level, &children[i], &children[j]) {
                        children[j].frame.at = at;
                        known[j] = true;
                        progress = true;
                        break;
                    }
                }
            }
        }
        assert!(known.iter().all(|&k| k), "level {level} kind {kind}: children do not touch");
        self.levels[level][kind].children = children;
    }

    /// Searches for a vertex shared by hats of two children, closest pairs first.
    /// Returns the split position of `b`, derived through `a`'s known one.
    fn touching(&self, child_level: i32, a: &Child, b: &Child) -> Option<Split> {
        let start_b = Frame { rotation: 0, flip: false, at: Split::default() };
        let a_node = Node { level: child_level, kind: a.kind, frame: a.frame, placed: a.frame };
        let b_node = Node { level: child_level, kind: b.kind, frame: start_b, placed: b.frame };
        let mut heap = BinaryHeap::new();
        heap.push(self.candidate(a_node, b_node));
        let mut budget = SEARCH_BUDGET;

        while let Some(Candidate { a, b, .. }) = heap.pop() {
            budget = budget.checked_sub(1)?;
            let (ca, ra) = self.node_bounds(a.level, a.kind, &a.placed);
            let (cb, rb) = self.node_bounds(b.level, b.kind, &b.placed);
            if (ca.0 - cb.0).hypot(ca.1 - cb.1) > ra + rb + 1e-9 {
                continue;
            }
            if a.level < 0 && b.level < 0 {
                for va in &self.outline {
                    let at_a = a.frame.apply(*va);
                    for vb in &self.outline {
                        if b.placed.apply(*vb).lattice() == at_a.lattice() {
                            // The hat in `b`, seen from `b` itself.
                            let inside = b.frame.apply(*vb);
                            let root = root_linear(&b.placed, &b.frame);
                            return Some(at_a.sub(root.linear_split(inside)));
                        }
                    }
                }
                continue;
            }
            // Splits the larger side.
            if a.level >= b.level {
                for child in &self.levels[a.level as usize][a.kind].children {
                    let next = Node {
                        level: a.level - 1,
                        kind: child.kind,
                        frame: a.frame.then(&child.frame),
                        placed: a.placed.then(&child.frame),
                    };
                    heap.push(self.candidate(next, b));
                }
            } else {
                for child in &self.levels[b.level as usize][b.kind].children {
                    let next = Node {
                        level: b.level - 1,
                        kind: child.kind,
                        frame: b.frame.then(&child.frame),
                        placed: b.placed.then(&child.frame),
                    };
                    heap.push(self.candidate(a, next));
                }
            }
        }
        None
    }

    fn candidate(&self, a: Node, b: Node) -> Candidate {
        let (ca, _) = self.node_bounds(a.level, a.kind, &a.placed);
        let (cb, _) = self.node_bounds(b.level, b.kind, &b.placed);
        Candidate { distance: (ca.0 - cb.0).hypot(ca.1 - cb.1), a, b }
    }
}

impl Default for Hierarchy {
    fn default() -> Self {
        Self::new()
    }
}

/// The linear part of the frame that takes `inside` to `placed`.
fn root_linear(placed: &Frame, inside: &Frame) -> Frame {
    let flip = placed.flip != inside.flip;
    let rotation = if flip {
        (placed.rotation + inside.rotation) % 6
    } else {
        (placed.rotation + 6 - inside.rotation) % 6
    };
    Frame { rotation, flip, at: Split::default() }
}

fn frame_point(frame: &Frame, p: (f64, f64)) -> (f64, f64) {
    let y = if frame.flip { -p.1 } else { p.1 };
    let angle = frame.rotation as f64 * std::f64::consts::PI / 3.0;
    let (s, c) = angle.sin_cos();
    let at = frame.at.lattice();
    (c * p.0 - s * y + at.x(), s * p.0 + c * y + at.y())
}

/// Calls `visit` with every hat inside a metatile, in document order.
pub fn for_each_hat(hierarchy: &Hierarchy, level: usize, kind: usize, frame: &Frame, visit: &mut impl FnMut(&Frame)) {
    for child in &hierarchy.levels[level][kind].children {
        let next = frame.then(&child.frame);
        if level == 0 {
            visit(&next);
        } else {
            for_each_hat(hierarchy, level - 1, child.kind, &next, visit);
        }
    }
}

/// Edge `i` of a hat, oriented so that the hat lies on the same side of it whether
/// or not the hat is mirrored. Two hats sharing an edge then have opposite ones.
pub fn directed_edge(frame: &Frame, points: &[Hex; 14], i: usize) -> (Hex, Hex) {
    let (a, b) = (points[i], points[(i + 1) % 14]);
    if frame.flip { (b, a) } else { (a, b) }
}

/// Lattice points of a hat, keyed for adjacency tests.
pub fn hat_points(hierarchy: &Hierarchy, frame: &Frame) -> [Hex; 14] {
    let mut out = [Hex::default(); 14];
    for (i, v) in hierarchy.outline.iter().enumerate() {
        out[i] = frame.apply(*v).lattice();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn hats_of(h: &Hierarchy, level: usize, kind: usize) -> Vec<Frame> {
        let mut out = Vec::new();
        for_each_hat(h, level, kind, &Frame::default(), &mut |f| out.push(*f));
        out
    }

    #[test]
    fn outline_closes_and_matches_the_hat() {
        let outline = hat_outline();
        // The reference's hexPt coordinates, with the midpoint of the long edge.
        let expected = [
            (0, 0), (-1, -1), (0, -2), (1, -2), (2, -2), (2, -1), (4, -2), (5, -1),
            (4, 0), (3, 0), (2, 2), (0, 3), (0, 2), (-1, 2),
        ];
        for (v, e) in outline.iter().zip(expected) {
            assert_eq!(v.lattice(), Hex::new(e.0, e.1));
        }
    }

    #[test]
    fn all_hats_stay_on_the_lattice() {
        let h = Hierarchy::new();
        // Rounding noise at coordinates in the millions, far from the 0.5 at which
        // a position would snap to the wrong lattice point.
        assert!(h.worst_snap < 0.01, "worst deviation {}", h.worst_snap);
    }

    #[test]
    fn area_grows_by_phi_to_the_fourth() {
        let h = Hierarchy::new();
        let phi4 = ((1.0 + 5f64.sqrt()) / 2.0).powi(4);
        for level in 6..LEVELS {
            let ratio = h.levels[level][H].hats / h.levels[level - 1][H].hats;
            assert!((ratio - phi4).abs() < 0.01, "level {level}: ratio {ratio}");
        }
    }

    /// Every hat edge must be shared by exactly two hats, or lie on the patch
    /// boundary; hats must not coincide.
    #[test]
    fn hats_fit_together_without_overlap() {
        let h = Hierarchy::new();
        let hats = hats_of(&h, 4, H);
        assert!(hats.len() > 1000);
        let unique: HashSet<_> = hats.iter().map(|f| (f.rotation, f.flip, f.at.lattice())).collect();
        assert_eq!(unique.len(), hats.len());

        let mut edges: HashMap<(Hex, Hex), u32> = HashMap::new();
        for f in &hats {
            let points = hat_points(&h, f);
            for i in 0..14 {
                // The same directed edge twice would mean overlapping hats.
                *edges.entry(directed_edge(f, &points, i)).or_default() += 1;
            }
        }
        assert!(edges.values().all(|&n| n == 1), "overlapping edges");
        let inner = edges.keys().filter(|(a, b)| edges.contains_key(&(*b, *a))).count();
        assert!(inner * 10 > edges.len() * 8, "only {inner} of {} edges shared", edges.len());
    }

    /// This is what morphing relies on: where hats meet on the lattice, they must
    /// also agree on the split position, or Tile(a, b) would tear apart.
    #[test]
    fn hats_agree_on_split_positions() {
        let h = Hierarchy::new();
        for kind in [H, T, P, F] {
            let hats = hats_of(&h, 4, kind);
            let mut seen: HashMap<Hex, Split> = HashMap::new();
            for f in &hats {
                for v in &h.outline {
                    let at = f.apply(*v);
                    if let Some(previous) = seen.insert(at.lattice(), at) {
                        assert_eq!(previous, at, "kind {kind}");
                    }
                }
            }
        }
    }

    #[test]
    fn neighbour_counts_stay_small() {
        let h = Hierarchy::new();
        let hats = hats_of(&h, 5, H);
        let mut by_point: HashMap<Hex, Vec<usize>> = HashMap::new();
        let mut by_edge: HashMap<(Hex, Hex), usize> = HashMap::new();
        for (i, f) in hats.iter().enumerate() {
            let points = hat_points(&h, f);
            for k in 0..14 {
                by_point.entry(points[k]).or_default().push(i);
                by_edge.insert(directed_edge(f, &points, k), i);
            }
        }
        let mut vertex_max = 0;
        let mut edge_max = 0;
        let mut vertex_histogram: HashMap<usize, usize> = HashMap::new();
        for (i, f) in hats.iter().enumerate() {
            let points = hat_points(&h, f);
            let mut vertex = HashSet::new();
            let mut edge = HashSet::new();
            for k in 0..14 {
                vertex.extend(by_point[&points[k]].iter().copied());
                let (from, to) = directed_edge(f, &points, k);
                if let Some(&other) = by_edge.get(&(to, from)) {
                    edge.insert(other);
                }
            }
            vertex.remove(&i);
            vertex_max = vertex_max.max(vertex.len());
            edge_max = edge_max.max(edge.len());
            *vertex_histogram.entry(vertex.len()).or_default() += 1;
        }
        println!("hats {} edge max {edge_max} vertex max {vertex_max} histogram {vertex_histogram:?}", hats.len());
        println!(
            "hats per level: {:?}",
            (0..LEVELS).map(|l| h.levels[l][H].hats).collect::<Vec<_>>()
        );
        println!(
            "radius per level: {:?}",
            (0..LEVELS).map(|l| h.levels[l][H].radius.round()).collect::<Vec<_>>()
        );
        assert!(edge_max <= 12);
        assert!(vertex_max <= 20);
    }
}
