//! Looks for patterns that travel across the hat tiling under two-state rules. This
//! is the core of the command line tool in `bin/search.rs`, free of threads and files.
//!
//! Exact translation cannot exist on an aperiodic tiling, so what counts is how far
//! a pattern gets in rings, the graph distance over edges and corners, while it
//! stays small. Everything depends only on places in the tiling and on seeds, never
//! on cell ids, so results do not depend on the number of workers.

use crate::hat_world::{CellKey, HatWorld};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// Measured over 540,000 hats: a ring is 4.1 to 4.3 units wide once it is far from
/// the start, and at least 3.6 near it. A flight has to go by the widest, or it stops
/// short of its goal along the fast axes, which is where gliders fly.
pub const UNITS_PER_RING_MIN: f64 = 3.6;
pub const UNITS_PER_RING_FAR: f64 = 4.1;
pub const UNITS_PER_RING_MAX: f64 = 4.45;
/// Largest number of cells within 2 and 3 rings, measured.
const MAX_BALL: [u8; 4] = [0, 7, 22, 45];

/// Gridded here to see how much of the plane a pattern covers.
const AREA_BUCKET: f64 = 16.0;
/// A worker starts over with a fresh world past this many cells, for memory's sake.
const REBUILD_CELLS: usize = 400_000;
/// Enough for the largest patch: 400 rings are about 760,000 hats.
const VERIFY_MAX_CELLS: usize = 3_000_000;
const CLUSTER_GAP: f64 = 14.0;
/// A live cell this far from the start means something has left.
const DEPARTED: f64 = 60.0;
const CLUSTER_MIN_DISTANCE: f64 = 40.0;
const MAX_CLUSTERS: usize = 6;
const MEMO_LIMIT: usize = 8_000_000;
const CORRIDOR_MAX_PATHS: usize = 4000;
/// Once something is this far out, it has left, and flying it further here would
/// only generate field. About 50 rings.
const LATE_STOP_SPREAD: f64 = 200.0;

pub const TRAVELLER_RINGS: f64 = 30.0;
pub const GLIDER_RINGS: f64 = 300.0;
pub const PATCH_RINGS: [u32; 3] = [100, 200, 400];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hood {
    /// Edges and corners, the engine's usual neighbourhood.
    Moore,
    Edge,
    /// All cells within this many rings.
    Radius(u8),
}

impl Hood {
    pub fn parse(text: &str) -> Option<Hood> {
        match text {
            "moore" => Some(Hood::Moore),
            "edge" => Some(Hood::Edge),
            "r2" => Some(Hood::Radius(2)),
            "r3" => Some(Hood::Radius(3)),
            _ => None,
        }
    }
    pub fn name(self) -> String {
        match self {
            Hood::Moore => "moore".into(),
            Hood::Edge => "edge".into(),
            Hood::Radius(r) => format!("r{r}"),
        }
    }
    pub fn radius(self) -> u8 {
        match self {
            Hood::Radius(r) => r,
            _ => 1,
        }
    }
    /// The most neighbours a hat has in this neighbourhood.
    pub fn max_count(self) -> u8 {
        match self {
            Hood::Moore => 7,
            Hood::Edge => 6,
            Hood::Radius(r) => MAX_BALL[r as usize],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub birth: u128,
    pub survive: u128,
    pub name: String,
}

fn digits(mask: u128) -> String {
    (0..16).filter(|n| mask >> n & 1 == 1).map(|n| char::from_digit(n, 16).unwrap()).collect()
}

fn range_mask(low: u8, high: u8) -> u128 {
    (low..=high).fold(0, |mask, n| mask | 1u128 << n)
}

impl Rule {
    pub fn bits(birth: u16, survive: u16) -> Rule {
        Rule { birth: birth as u128, survive: survive as u128, name: format!("B{}/S{}", digits(birth as u128), digits(survive as u128)) }
    }

    pub fn range(radius: u8, birth: (u8, u8), survive: (u8, u8)) -> Rule {
        Rule {
            birth: range_mask(birth.0, birth.1),
            survive: range_mask(survive.0, survive.1),
            name: format!("R{radius}B{}-{}S{}-{}", birth.0, birth.1, survive.0, survive.1),
        }
    }

    /// `B36/S234` for moore and edge, `R2B5-7S3-8` for a radius.
    pub fn parse(text: &str, hood: Hood) -> Result<Rule, String> {
        let bad = || format!("cannot read the rule {text} for the neighbourhood {}", hood.name());
        let top = hood.max_count();
        if let Hood::Radius(radius) = hood {
            let rest = text.strip_prefix(&format!("R{radius}B")).ok_or_else(bad)?;
            let (b, s) = rest.split_once('S').ok_or_else(bad)?;
            let pair = |part: &str| -> Option<(u8, u8)> {
                let (low, high) = part.split_once('-')?;
                let pair = (low.parse().ok()?, high.parse().ok()?);
                (pair.0 <= pair.1 && pair.1 < 128).then_some(pair)
            };
            let (b, s) = (pair(b).ok_or_else(bad)?, pair(s).ok_or_else(bad)?);
            if b.0 == 0 {
                return Err(format!("{text}: birth from nothing would fill the unbounded field"));
            }
            return Ok(Rule::range(radius, b, s));
        }
        let rest = text.strip_prefix('B').ok_or_else(bad)?;
        let (b, s) = rest.split_once("/S").ok_or_else(bad)?;
        let mask = |part: &str| -> Option<u16> {
            part.chars().try_fold(0u16, |m, c| {
                let n = c.to_digit(16)? as u8;
                (n <= top).then(|| m | 1 << n)
            })
        };
        let (b, s) = (mask(b).ok_or_else(bad)?, mask(s).ok_or_else(bad)?);
        if b & 1 != 0 {
            return Err(format!("{text}: birth from nothing would fill the unbounded field"));
        }
        Ok(Rule::bits(b, s))
    }

    /// Every rule of a neighbourhood without B0, in a fixed order.
    pub fn all(hood: Hood) -> Vec<Rule> {
        let top = hood.max_count();
        if let Hood::Radius(radius) = hood {
            let mut rules = Vec::new();
            for bl in 1..=top {
                for bh in bl..=top {
                    for sl in 0..=top {
                        for sh in sl..=top {
                            rules.push(Rule::range(radius, (bl, bh), (sl, sh)));
                        }
                    }
                }
            }
            return rules;
        }
        let mut rules = Vec::with_capacity(1 << (2 * top + 1));
        for birth in 0u16..1 << top {
            for survive in 0u16..1 << (top + 1) {
                rules.push(Rule::bits(birth << 1, survive));
            }
        }
        rules
    }
}

pub fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// A seed for one thing, from the seed of the run and what that thing is.
pub fn mix_seed(parts: &[u64]) -> u64 {
    let mut h = 0x9E37_79B9_7F4A_7C15u64;
    for &part in parts {
        h = (h ^ part).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 29;
    }
    h
}

/// The generator of `HatWorld::soup`.
#[derive(Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng((seed ^ seed >> 32) as u32 | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() as u64 * n as u64 >> 32) as usize
    }
}

fn centroid(world: &HatWorld, cells: &[u32]) -> (f64, f64) {
    let (mut x, mut y) = (0.0, 0.0);
    for &c in cells {
        let p = world.center(c);
        x += p.0;
        y += p.1;
    }
    let n = cells.len().max(1) as f64;
    (x / n, y / n)
}

fn nearest_to(world: &HatWorld, cells: &[u32], p: (f64, f64)) -> Option<u32> {
    let far = |c: u32| {
        let q = world.center(c);
        (q.0 - p.0).hypot(q.1 - p.1)
    };
    cells.iter().copied().min_by(|&a, &b| far(a).total_cmp(&far(b)))
}

/// Generates and returns all cells within `rings` of a cell, the cell first.
pub fn ball(world: &mut HatWorld, cell: u32, rings: u32) -> Vec<u32> {
    let (x, y) = world.center(cell);
    world.load_around(x, y, (rings as f64 + 2.0) * 8.0);
    let mut found = vec![cell];
    let mut known: HashSet<u32> = HashSet::from([cell]);
    let mut from = 0;
    for _ in 0..rings {
        let to = found.len();
        for i in from..to {
            for &n in world.touching(found[i]) {
                if known.insert(n) {
                    found.push(n);
                }
            }
        }
        from = to;
    }
    found
}

/// Steps from one cell to another over edges and corners.
pub fn rings_between(world: &mut HatWorld, a: u32, b: u32) -> Option<u32> {
    let (pa, pb) = (world.center(a), world.center(b));
    let d = (pa.0 - pb.0).hypot(pa.1 - pb.1);
    world.load_around((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0, d * 0.6 + 30.0);
    let mut known: HashSet<u32> = HashSet::from([a]);
    let mut frontier = vec![a];
    let mut rings = 0;
    while !frontier.is_empty() {
        if frontier.contains(&b) {
            return Some(rings);
        }
        let mut next = Vec::new();
        for &c in &frontier {
            for &n in world.touching(c) {
                if known.insert(n) {
                    next.push(n);
                }
            }
        }
        frontier = next;
        rings += 1;
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    Died,
    Still,
    Oscillator(u32),
    /// More cells than allowed.
    Overpopulated,
    /// Reached a state some earlier seed of this rule has been through.
    Merged,
    /// No progress for too long.
    Stalled,
    OutOfGenerations,
    ReachedTarget,
    /// The world ran out of cells.
    Capped,
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub pop_cap: u32,
    /// For this many generations at the start the pattern may grow up to `transient_cap`.
    pub transient: u32,
    pub transient_cap: u32,
    pub patience: u32,
    pub max_generations: u32,
    /// Stops when the centre of the pattern is this far from where it started.
    pub stop_reach: f64,
    /// Stops when any live cell is this far from where the pattern started.
    pub stop_spread: f64,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub fate: Fate,
    pub generations: u32,
    pub min_pop: u32,
    pub max_pop: u32,
    pub start: (f64, f64),
    pub start_cell: u32,
    /// Largest distance of the centre from the start, and the cell nearest it there.
    pub reach: f64,
    pub reach_cell: u32,
    pub reach_generation: u32,
    pub angle: f64,
    /// Largest distance of any live cell from the start.
    pub spread: f64,
    /// Largest distance of a live cell from the centre, per doubling span of time.
    pub extents: Vec<f64>,
    pub last: Vec<u32>,
}

impl Run {
    /// Whether the pattern's size never grew by much over what it had before.
    pub fn region_stable(&self) -> bool {
        let mut largest = match self.extents.first() {
            Some(&first) => first,
            None => return true,
        };
        for &extent in &self.extents[1..] {
            if extent > 1.25 * largest + 6.0 {
                return false;
            }
            largest = largest.max(extent);
        }
        true
    }
}

/// Runs what is on the field. `trail` is only there to keep its memory.
pub fn run(world: &mut HatWorld, limits: &Limits, memo: Option<&HashSet<u64>>, trail: &mut HashMap<u64, u32>) -> Run {
    trail.clear();
    trail.insert(world.field_hash(), 0);
    let start = centroid(world, world.live());
    let start_cell = nearest_to(world, world.live(), start).unwrap_or(0);
    let start_pop = world.population as u32;
    let mut out = Run {
        fate: Fate::OutOfGenerations,
        generations: 0,
        min_pop: start_pop,
        max_pop: start_pop,
        start,
        start_cell,
        reach: 0.0,
        reach_cell: start_cell,
        reach_generation: 0,
        angle: 0.0,
        spread: 0.0,
        extents: Vec::new(),
        last: Vec::new(),
    };
    let (mut best, mut stale) = (0.0f64, 0u32);
    let mut generation = 0;
    while generation < limits.max_generations {
        world.step();
        generation += 1;
        let pop = world.population as u32;
        out.min_pop = out.min_pop.min(pop);
        out.max_pop = out.max_pop.max(pop);
        out.generations = generation;
        if pop == 0 {
            out.fate = Fate::Died;
            break;
        }
        if pop > if generation <= limits.transient { limits.transient_cap } else { limits.pop_cap } {
            out.fate = Fate::Overpopulated;
            break;
        }
        if world.capped {
            out.fate = Fate::Capped;
            break;
        }
        let hash = world.field_hash();
        if let Some(earlier) = trail.insert(hash, generation) {
            let period = generation - earlier;
            out.fate = if period == 1 { Fate::Still } else { Fate::Oscillator(period) };
            break;
        }
        if memo.is_some_and(|m| m.contains(&hash)) {
            out.fate = Fate::Merged;
            break;
        }
        // Each cell costs little, but this is the search's innermost loop.
        if generation % 4 != 0 {
            continue;
        }
        let center = centroid(world, world.live());
        let (dx, dy) = (center.0 - start.0, center.1 - start.1);
        let reach = dx.hypot(dy);
        let (mut extent, mut spread) = (0.0f64, 0.0f64);
        for &c in world.live() {
            let p = world.center(c);
            extent = extent.max((p.0 - center.0).hypot(p.1 - center.1));
            spread = spread.max((p.0 - start.0).hypot(p.1 - start.1));
        }
        out.spread = out.spread.max(spread);
        if reach > out.reach {
            out.reach = reach;
            out.reach_generation = generation;
            out.angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
            out.reach_cell = nearest_to(world, world.live(), center).unwrap_or(start_cell);
        }
        if generation > limits.transient {
            let span = ((generation - limits.transient) / 16 + 1).ilog2() as usize;
            if out.extents.len() <= span {
                out.extents.resize(span + 1, 0.0);
            }
            out.extents[span] = out.extents[span].max(extent);
        }
        if reach >= limits.stop_reach || out.spread >= limits.stop_spread {
            out.fate = Fate::ReachedTarget;
            break;
        }
        let progress = reach.max(out.spread);
        if progress > best + 2.0 {
            best = progress;
            stale = 0;
        } else {
            stale += 4;
            if generation > limits.transient && stale >= limits.patience {
                out.fate = Fate::Stalled;
                break;
            }
        }
    }
    out.last = world.live().to_vec();
    out
}

/// Calls `visit` with every connected set of `size` cells holding `root`, each once.
/// Stops when `visit` returns false.
pub fn connected_sets(adjacent: &HashMap<u32, Vec<u32>>, root: u32, size: usize, visit: &mut impl FnMut(&[u32]) -> bool) -> bool {
    fn extend(adjacent: &HashMap<u32, Vec<u32>>, set: &mut Vec<u32>, open: &[u32], blocked: &mut HashSet<u32>, size: usize, visit: &mut impl FnMut(&[u32]) -> bool) -> bool {
        if set.len() == size {
            return visit(set);
        }
        let mut added = Vec::new();
        let mut keep_going = true;
        for (i, &cell) in open.iter().enumerate() {
            // Sets with this cell are dealt with now, so later ones must go without it.
            blocked.insert(cell);
            added.push(cell);
            set.push(cell);
            let mut next: Vec<u32> = open[i + 1..].to_vec();
            let mut fresh = Vec::new();
            if let Some(around) = adjacent.get(&cell) {
                for &n in around {
                    if !blocked.contains(&n) && !set.contains(&n) && !next.contains(&n) {
                        next.push(n);
                        fresh.push(n);
                    }
                }
            }
            keep_going = extend(adjacent, set, &next, blocked, size, visit);
            set.pop();
            if !keep_going {
                break;
            }
        }
        for cell in added {
            blocked.remove(&cell);
        }
        keep_going
    }
    let mut set = vec![root];
    let mut blocked = HashSet::from([root]);
    let open: Vec<u32> = adjacent.get(&root).cloned().unwrap_or_default();
    extend(adjacent, &mut set, &open, &mut blocked, size, visit)
}

/// Where in the tiling seeds get tried.
#[derive(Clone, Debug)]
pub struct Position {
    /// H, T, P or F for the metatile of a normal hat, or anti for a mirrored one.
    pub class: &'static str,
    pub key: CellKey,
}

pub const POSITION_CLASSES: [&str; 5] = ["H", "T", "P", "F", "anti"];
const POSITION_SPACING: f64 = 60.0;
const POSITION_REGION: f64 = 900.0;

fn class_of(world: &HatWorld, cell: u32) -> usize {
    if world.is_mirrored(cell) { 4 } else { world.metatile(cell) as usize }
}

/// Places of each class in turn, nearest to the middle first and well apart.
pub fn select_positions(world: &mut HatWorld, count: usize) -> Vec<Position> {
    world.load_around(0.0, 0.0, POSITION_REGION);
    let mut by_class: [Vec<u32>; 5] = Default::default();
    for cell in 0..world.cell_count() as u32 {
        by_class[class_of(world, cell)].push(cell);
    }
    let distance = |w: &HatWorld, c: u32| {
        let p = w.center(c);
        p.0.hypot(p.1)
    };
    for list in &mut by_class {
        list.sort_by(|&a, &b| distance(world, a).total_cmp(&distance(world, b)).then_with(|| world.key_of(a).cmp(&world.key_of(b))));
    }
    let mut chosen: Vec<(usize, u32)> = Vec::new();
    let mut cursor = [0usize; 5];
    let mut class = 0;
    let mut exhausted = 0;
    while chosen.len() < count && exhausted < 5 {
        let list = &by_class[class];
        let mut found = false;
        while cursor[class] < list.len() {
            let cell = list[cursor[class]];
            cursor[class] += 1;
            let p = world.center(cell);
            let free = chosen.iter().all(|&(_, other)| {
                let q = world.center(other);
                (p.0 - q.0).hypot(p.1 - q.1) >= POSITION_SPACING
            });
            if free {
                chosen.push((class, cell));
                found = true;
                break;
            }
        }
        exhausted = if found { 0 } else { exhausted + 1 };
        class = (class + 1) % 5;
    }
    chosen.into_iter().map(|(class, cell)| Position { class: POSITION_CLASSES[class], key: world.key_of(cell) }).collect()
}

/// A strip between two parallel lines of mirrored hats.
#[derive(Clone, Debug)]
pub struct Corridor {
    /// Direction of the lines in degrees, below 180.
    pub axis: f64,
    pub gap: f64,
    pub overlap: f64,
    /// Hats of the strip, walls included.
    pub cells: Vec<CellKey>,
    /// Hats to start chains from, around the middle of the strip.
    pub starts: Vec<CellKey>,
}

/// Mirrored hats of one orientation line up along these directions, 22 or 34 units
/// apart, the fast axes of the tiling's graph metric. Measured over 11,000. Hats 10
/// units apart are pairs of different orientation, and make no lines.
const LINE_STEPS: [f64; 2] = [22.0, 34.0];
const LINE_STEP_TOLERANCE: f64 = 1.0;
const LINE_AXES: [f64; 3] = [46.0, 106.0, 166.0];
const LINE_ANGLE_TOLERANCE: f64 = 2.5;
const LINE_MIN_HATS: usize = 3;
const CORRIDOR_MAX_GAP: f64 = 60.0;
const CORRIDOR_MIN_OVERLAP: f64 = 20.0;
const CORRIDOR_REGION: f64 = 700.0;
/// Part of a corridor along its length where chains may start.
const CORRIDOR_START_SPAN: f64 = 6.0;
const CORRIDOR_MAX_LENGTH: f64 = 60.0;
const CORRIDOR_SPACING: f64 = 40.0;

pub struct CorridorStats {
    /// Greatest distance of a hat from the middle of its line.
    pub wobbles: Vec<f64>,
    pub lines: Vec<usize>,
    pub gaps: Vec<f64>,
    pub corridors: usize,
}

struct Line {
    along: (f64, f64),
    across: f64,
}

/// Finds the lines of mirrored hats, then the corridors between neighbouring ones.
pub fn find_corridors(world: &mut HatWorld, count: usize) -> (Vec<Corridor>, CorridorStats) {
    world.load_around(0.0, 0.0, CORRIDOR_REGION);
    let mirrored: Vec<u32> = (0..world.cell_count() as u32).filter(|&c| world.is_mirrored(c)).collect();
    let mut stats = CorridorStats { wobbles: Vec::new(), lines: Vec::new(), gaps: Vec::new(), corridors: 0 };
    let mut sites: Vec<(f64, f64, f64, f64, f64, f64, f64)> = Vec::new();
    for axis in LINE_AXES {
        let (u, v) = {
            let (s, c) = axis.to_radians().sin_cos();
            ((c, s), (-s, c))
        };
        let along = |p: (f64, f64)| p.0 * u.0 + p.1 * u.1;
        let across = |p: (f64, f64)| p.0 * v.0 + p.1 * v.1;
        let mut links: HashMap<u32, Vec<u32>> = HashMap::new();
        // Sorted along the axis, partners are found by looking a step ahead.
        let mut order: Vec<(f64, u32)> = mirrored.iter().map(|&c| (along(world.center(c)), c)).collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (i, &(_, a)) in order.iter().enumerate() {
            let p = world.center(a);
            for &(_, b) in &order[i + 1..] {
                let q = world.center(b);
                let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                if along((dx, dy)) > LINE_STEPS[1] + LINE_STEP_TOLERANCE {
                    break;
                }
                let d = dx.hypot(dy);
                let angle = dy.atan2(dx).to_degrees().rem_euclid(180.0);
                let spaced = LINE_STEPS.iter().any(|step| (d - step).abs() <= LINE_STEP_TOLERANCE);
                if spaced && world.orientation(a) == world.orientation(b) && (angle - axis).abs() <= LINE_ANGLE_TOLERANCE {
                    links.entry(a).or_default().push(b);
                    links.entry(b).or_default().push(a);
                }
            }
        }
        let mut seen: HashSet<u32> = HashSet::new();
        let mut lines: Vec<Line> = Vec::new();
        for &(_, a) in &order {
            if !links.contains_key(&a) || !seen.insert(a) {
                continue;
            }
            let mut members = vec![a];
            let mut i = 0;
            while i < members.len() {
                for &n in &links[&members[i]] {
                    if seen.insert(n) {
                        members.push(n);
                    }
                }
                i += 1;
            }
            stats.lines.push(members.len());
            if members.len() < LINE_MIN_HATS {
                continue;
            }
            let points: Vec<(f64, f64)> = members.iter().map(|&c| world.center(c)).collect();
            let low = points.iter().map(|&p| along(p)).fold(f64::MAX, f64::min);
            let high = points.iter().map(|&p| along(p)).fold(f64::MIN, f64::max);
            let middle = points.iter().map(|&p| across(p)).sum::<f64>() / points.len() as f64;
            let wobble = points.iter().map(|&p| (across(p) - middle).abs()).fold(0.0, f64::max);
            stats.wobbles.push(wobble);
            lines.push(Line { along: (low, high), across: middle });
        }
        for line in &lines {
            let neighbour = lines
                .iter()
                .filter(|o| o.across > line.across + 2.0 && o.along.1.min(line.along.1) - o.along.0.max(line.along.0) >= CORRIDOR_MIN_OVERLAP)
                .min_by(|a, b| a.across.total_cmp(&b.across));
            if let Some(other) = neighbour {
                let gap = other.across - line.across;
                stats.gaps.push(gap);
                if gap <= CORRIDOR_MAX_GAP {
                    let low = line.along.0.max(other.along.0);
                    let high = line.along.1.min(other.along.1).min(low + CORRIDOR_MAX_LENGTH);
                    sites.push((axis, gap, low, high, line.across, u.0, u.1));
                }
            }
        }
    }
    stats.corridors = sites.len();
    // Nearest to the middle first, and well apart.
    let middle_of = |s: &(f64, f64, f64, f64, f64, f64, f64)| {
        let (a, m) = ((s.2 + s.3) / 2.0, s.4 + s.1 / 2.0);
        (a * s.5 - m * s.6, a * s.6 + m * s.5)
    };
    sites.sort_by(|a, b| {
        let (p, q) = (middle_of(a), middle_of(b));
        p.0.hypot(p.1).total_cmp(&q.0.hypot(q.1)).then(a.0.total_cmp(&b.0))
    });
    let mut chosen: Vec<(f64, f64, f64, f64, f64, f64, f64)> = Vec::new();
    for site in sites {
        if chosen.len() >= count {
            break;
        }
        let p = middle_of(&site);
        if chosen.iter().all(|o| {
            let q = middle_of(o);
            (p.0 - q.0).hypot(p.1 - q.1) >= CORRIDOR_SPACING
        }) {
            chosen.push(site);
        }
    }
    let mut corridors = Vec::new();
    for &(axis, gap, low, high, base, cx, sy) in &chosen {
        let (u, v) = ((cx, sy), (-sy, cx));
        let mut cells = Vec::new();
        let mut starts = Vec::new();
        let middle = (low + high) / 2.0;
        for cell in 0..world.cell_count() as u32 {
            let p = world.center(cell);
            let along = p.0 * u.0 + p.1 * u.1;
            let across = p.0 * v.0 + p.1 * v.1 - base;
            if across >= -1.5 && across <= gap + 1.5 && along >= low && along <= high {
                cells.push(world.key_of(cell));
                if (along - middle).abs() <= CORRIDOR_START_SPAN {
                    starts.push(world.key_of(cell));
                }
            }
        }
        cells.sort();
        starts.sort();
        corridors.push(Corridor { axis, gap, overlap: high - low, cells, starts });
    }
    (corridors, stats)
}

#[derive(Clone, Debug, PartialEq)]
pub enum RuleClass {
    Explosive,
    Dies,
    Stable,
    Chaotic,
}

impl RuleClass {
    pub fn name(&self) -> &'static str {
        match self {
            RuleClass::Explosive => "explosive",
            RuleClass::Dies => "dies",
            RuleClass::Stable => "stable",
            RuleClass::Chaotic => "chaotic",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SoupScreen {
    pub class: RuleClass,
    pub seed: u32,
    pub position: usize,
    pub start_pop: u32,
    pub peak: u32,
    pub generations: u32,
    pub period: u32,
}

#[derive(Clone, Debug)]
pub struct SeedPlan {
    pub polyhat: Option<(usize, usize)>,
    pub corridor: bool,
    /// Rings of the soups, each with all densities.
    pub soup: Option<(u32, u32)>,
    pub mutate: bool,
    /// Polyhat sizes and soup rings that may burn for a long time, see `evaluate_late`.
    pub late: Option<(usize, usize)>,
    pub late_soup: Option<(u32, u32)>,
}

#[derive(Clone)]
pub struct Config {
    pub hood: Hood,
    pub plan: SeedPlan,
    pub positions: Vec<Position>,
    pub corridors: Vec<Corridor>,
    pub master_seed: u64,
    pub pop_factor: u32,
    pub pop_floor: u32,
    pub max_pop: u32,
    pub patience: u32,
    pub max_generations: u32,
    pub screen_soups: u32,
    pub screen_generations: u32,
    pub screen_rings: u32,
    /// Which classes get a search after the screening.
    pub keep: Vec<RuleClass>,
    pub soups_each: u32,
    pub densities: Vec<f64>,
    pub max_seeds_per_rule: u64,
    pub mutate_top: usize,
    pub mutate_trials: u32,
    pub late_generations: u32,
    pub late_max_pop: u32,
}

impl Config {
    /// The smallest glider known on the hat starts from 2 cells and reaches 16, so a
    /// limit of 3 times the start alone would reject the very thing being looked for.
    pub fn pop_cap(&self, start: u32) -> u32 {
        (start * self.pop_factor).max(self.pop_floor).min(self.max_pop).max(1)
    }
}

#[derive(Clone, Debug)]
pub struct PatchCheck {
    pub rings: u32,
    pub passed: bool,
    pub reached: u32,
    pub generations: u32,
    pub note: &'static str,
}

#[derive(Clone, Debug)]
pub struct Find {
    pub category: &'static str,
    pub kind: &'static str,
    pub cells: Vec<CellKey>,
    /// For a pattern cut out of a larger run: what that run started from.
    pub derived_from: Option<Vec<CellKey>>,
    pub derived_generation: u32,
    pub position: Option<usize>,
    pub position_class: &'static str,
    pub soup_seed: Option<u32>,
    pub soup_rings: u32,
    pub soup_density: f64,
    pub mutation_seed: Option<u32>,
    pub reach_rings: f64,
    pub reach_units: f64,
    pub generations: u32,
    pub angle: f64,
    pub min_pop: u32,
    pub max_pop: u32,
    pub period: u32,
    pub occurrences: u64,
    pub patches: Vec<PatchCheck>,
    pub center: (f64, f64),
}

#[derive(Default, Clone, Debug)]
pub struct RuleStats {
    pub seeds: u64,
    pub died: u64,
    pub still: u64,
    pub oscillators: u64,
    pub overpopulated: u64,
    pub merged: u64,
    pub stalled: u64,
    pub out_of_generations: u64,
    pub truncated: bool,
    pub best_reach_units: f64,
    pub cuts_tried: u64,
    /// Patterns that mutation got further than they had come, and the furthest of them.
    pub mutations_improved: u64,
    pub mutation_reach_units: f64,
    /// The longest run of a late seed.
    pub late_generations_longest: u32,
}

#[derive(Clone, Debug)]
pub struct RuleReport {
    pub class: RuleClass,
    pub screens: Vec<SoupScreen>,
    pub searched: bool,
    pub stats: RuleStats,
    pub finds: Vec<Find>,
}

/// What a seed was, for reporting.
#[derive(Clone)]
struct Origin {
    kind: &'static str,
    position: Option<usize>,
    position_class: &'static str,
    soup_seed: Option<u32>,
    soup_rings: u32,
    soup_density: f64,
    mutation_seed: Option<u32>,
}

impl Origin {
    fn plain(kind: &'static str, position: Option<usize>, position_class: &'static str) -> Origin {
        Origin { kind, position, position_class, soup_seed: None, soup_rings: 0, soup_density: 0.0, mutation_seed: None }
    }
}

/// One worker's world, and all that goes with searching on it.
pub struct Searcher<'a> {
    pub config: &'a Config,
    world: HatWorld,
    verify_lock: &'a Mutex<()>,
    trail: HashMap<u64, u32>,
    memo: HashSet<u64>,
    /// Cell ids of positions and corridors, which hold until the world is rebuilt.
    position_cells: Vec<u32>,
    corridor_cells: Vec<(Vec<u32>, Vec<u32>)>,
    /// Where the seed count stops the current family of seeds. Each family has a
    /// budget of its own, or polyhats alone would use up everything on the very
    /// rules worth searching, and the corridors would never get their turn.
    family_end: u64,
}

fn new_world(hood: Hood) -> HatWorld {
    let mut world = HatWorld::new();
    world.observing = false;
    world.set_max_cells(VERIFY_MAX_CELLS);
    if hood == Hood::Edge {
        world.set_edges_only(true);
    }
    world
}

/// Sorted, so that a list of cells reads the same whatever their ids are.
fn keys_of(world: &HatWorld, cells: &[u32]) -> Vec<CellKey> {
    let mut keys: Vec<CellKey> = cells.iter().map(|&c| world.key_of(c)).collect();
    keys.sort();
    keys
}

struct Tally {
    finds: HashMap<String, usize>,
    list: Vec<Find>,
    tops: Vec<(f64, Vec<u32>, Origin)>,
}

impl<'a> Searcher<'a> {
    pub fn new(config: &'a Config, verify_lock: &'a Mutex<()>) -> Searcher<'a> {
        let mut searcher = Searcher {
            config,
            world: new_world(config.hood),
            verify_lock,
            trail: HashMap::new(),
            memo: HashSet::new(),
            position_cells: Vec::new(),
            corridor_cells: Vec::new(),
            family_end: 0,
        };
        searcher.locate();
        searcher
    }

    fn locate(&mut self) {
        let world = &mut self.world;
        self.position_cells = self.config.positions.iter().map(|p| world.cell_of(p.key).expect("position exists")).collect();
        self.corridor_cells = self
            .config
            .corridors
            .iter()
            .map(|c| {
                (
                    c.cells.iter().map(|&k| world.cell_of(k).expect("corridor cell exists")).collect(),
                    c.starts.iter().map(|&k| world.cell_of(k).expect("corridor cell exists")).collect(),
                )
            })
            .collect();
    }

    fn limits(&self, start: u32, transient: u32) -> Limits {
        Limits {
            pop_cap: self.config.pop_cap(start),
            transient,
            transient_cap: (start * 4).max(100),
            patience: self.config.patience,
            max_generations: self.config.max_generations,
            stop_reach: GLIDER_RINGS * UNITS_PER_RING_MAX,
            stop_spread: f64::INFINITY,
        }
    }

    /// Room to burn for a long time. Nothing here counts as flying by itself.
    fn late_limits(&self) -> Limits {
        Limits {
            pop_cap: self.config.late_max_pop,
            transient: 0,
            transient_cap: self.config.late_max_pop,
            patience: self.config.late_generations,
            max_generations: self.config.late_generations,
            stop_reach: f64::INFINITY,
            stop_spread: LATE_STOP_SPREAD,
        }
    }

    pub fn search_rule(&mut self, rule: &Rule) -> RuleReport {
        if self.world.cell_count() > REBUILD_CELLS {
            self.world = new_world(self.config.hood);
            self.locate();
        }
        self.world.reset_when_ruled();
        self.world.set_masks_when_empty(rule.birth, rule.survive, self.config.hood.radius());
        self.memo.clear();
        let rule_seed = mix_seed(&[self.config.master_seed, fnv(&rule.name)]);

        let screens: Vec<SoupScreen> = (0..self.config.screen_soups as u64).map(|i| self.screen_soup(rule_seed, i)).collect();
        let class = if screens.iter().any(|s| s.class == RuleClass::Explosive) {
            RuleClass::Explosive
        } else if screens.iter().all(|s| s.class == RuleClass::Dies) {
            RuleClass::Dies
        } else if screens.iter().all(|s| s.class != RuleClass::Chaotic) {
            RuleClass::Stable
        } else {
            RuleClass::Chaotic
        };
        let mut stats = RuleStats::default();
        let mut tally = Tally { finds: HashMap::new(), list: Vec::new(), tops: Vec::new() };
        let searched = self.config.keep.contains(&class);
        if searched {
            self.search_seeds(rule, rule_seed, &mut stats, &mut tally);
            // The field ended somewhere during this rule, so later seeds were cut short.
            stats.truncated |= self.world.capped;
        }
        self.world.reset_when_ruled();
        RuleReport { class, screens, searched, stats, finds: tally.list }
    }

    fn soup_cells(&mut self, position: usize, rings: u32, density: f64, seed: u32) -> Vec<u32> {
        let around = self.position_cells[position];
        let mut cells = ball(&mut self.world, around, rings);
        cells.sort_by_key(|&c| self.world.key_of(c));
        let threshold = (density.clamp(0.0, 1.0) * u32::MAX as f64) as u32;
        let mut rng = Rng::new(seed as u64);
        cells.retain(|_| rng.next() < threshold);
        cells
    }

    fn occupied(&self) -> usize {
        let buckets: HashSet<(i64, i64)> = self
            .world
            .live()
            .iter()
            .map(|&c| {
                let p = self.world.center(c);
                ((p.0 / AREA_BUCKET).floor() as i64, (p.1 / AREA_BUCKET).floor() as i64)
            })
            .collect();
        buckets.len()
    }

    /// A small random disc, watched for a short time. A growing bounding box alone
    /// says nothing: two gliders flying apart have one, and they must not be thrown
    /// out. So what counts is how much of the plane is actually occupied.
    fn screen_soup(&mut self, rule_seed: u64, index: u64) -> SoupScreen {
        let position = index as usize % self.position_cells.len();
        let seed = mix_seed(&[rule_seed, 1, index]) as u32;
        let cells = self.soup_cells(position, self.config.screen_rings, 0.5, seed);
        for &c in &cells {
            self.world.set_alive(c);
        }
        let start_pop = self.world.population as u32;
        let limit = start_pop * 4;
        let mut out = SoupScreen { class: RuleClass::Chaotic, seed, position, start_pop, peak: start_pop, generations: 0, period: 0 };
        let area_start = self.occupied();
        let mut area_at = [0usize; 2];
        self.trail.clear();
        self.trail.insert(self.world.field_hash(), 0);
        let total = self.config.screen_generations;
        for generation in 1..=total {
            self.world.step();
            out.generations = generation;
            let pop = self.world.population as u32;
            out.peak = out.peak.max(pop);
            if pop == 0 {
                out.class = RuleClass::Dies;
                break;
            }
            if pop > limit {
                out.class = RuleClass::Explosive;
                break;
            }
            if let Some(earlier) = self.trail.insert(self.world.field_hash(), generation) {
                out.class = RuleClass::Stable;
                out.period = generation - earlier;
                break;
            }
            if generation == total / 4 {
                area_at[0] = self.occupied();
            } else if generation == total / 2 {
                area_at[1] = self.occupied();
            } else if generation == total {
                let area = self.occupied();
                let growing = area as f64 > 2.0 * area_at[1] as f64 && area_at[1] as f64 > 1.3 * area_at[0] as f64 && area >= 2 * area_start;
                if growing {
                    out.class = RuleClass::Explosive;
                }
            }
        }
        self.world.reset_when_ruled();
        out
    }

    fn search_seeds(&mut self, rule: &Rule, rule_seed: u64, stats: &mut RuleStats, tally: &mut Tally) {
        let plan = self.config.plan.clone();
        let budget = self.config.max_seeds_per_rule;
        if let Some((low, high)) = plan.polyhat {
            self.family_end = stats.seeds + budget;
            for size in low..=high {
                for position in 0..self.position_cells.len() {
                    if !self.polyhats(rule, position, size, "polyhat", stats, tally) {
                        stats.truncated = true;
                        break;
                    }
                }
            }
        }
        if plan.corridor {
            self.family_end = stats.seeds + budget;
            for index in 0..self.corridor_cells.len() {
                self.corridor_seeds(rule, index, stats, tally);
            }
        }
        self.memo.clear();
        if let Some((low, high)) = plan.soup {
            self.family_end = stats.seeds + budget;
            let mut turn = 0;
            for rings in low..=high {
                for (d, &density) in self.config.densities.clone().iter().enumerate() {
                    for i in 0..self.config.soups_each {
                        if stats.seeds >= self.family_end {
                            stats.truncated = true;
                            break;
                        }
                        let position = turn % self.position_cells.len();
                        turn += 1;
                        let seed = mix_seed(&[rule_seed, 2, rings as u64, d as u64, i as u64]) as u32;
                        let cells = self.soup_cells(position, rings, density, seed);
                        if cells.is_empty() {
                            continue;
                        }
                        let origin = Origin {
                            soup_seed: Some(seed),
                            soup_rings: rings,
                            soup_density: density,
                            ..Origin::plain("soup", Some(position), self.config.positions[position].class)
                        };
                        // A soup needs time to burn down to what it leaves behind.
                        self.evaluate(rule, &origin, &cells, 200, false, None, stats, tally);
                    }
                }
            }
        }
        if plan.mutate {
            self.mutate(rule, rule_seed, stats, tally);
        }
        // Their limits differ, so what the others have seen says nothing about them.
        self.memo.clear();
        if let Some((low, high)) = plan.late {
            self.family_end = stats.seeds + budget;
            for size in low..=high {
                for position in 0..self.position_cells.len() {
                    if !self.polyhats(rule, position, size, "late", stats, tally) {
                        stats.truncated = true;
                        break;
                    }
                }
            }
        }
        if let Some((low, high)) = plan.late_soup {
            self.family_end = stats.seeds + budget;
            let mut turn = 0;
            for rings in low..=high {
                for (d, &density) in self.config.densities.clone().iter().enumerate() {
                    for i in 0..self.config.soups_each {
                        if stats.seeds >= self.family_end {
                            stats.truncated = true;
                            break;
                        }
                        let position = turn % self.position_cells.len();
                        turn += 1;
                        let seed = mix_seed(&[rule_seed, 4, rings as u64, d as u64, i as u64]) as u32;
                        let cells = self.soup_cells(position, rings, density, seed);
                        if cells.is_empty() {
                            continue;
                        }
                        let origin = Origin {
                            soup_seed: Some(seed),
                            soup_rings: rings,
                            soup_density: density,
                            ..Origin::plain("late", Some(position), self.config.positions[position].class)
                        };
                        self.evaluate_late(rule, &origin, &cells, false, stats, tally);
                    }
                }
            }
        }
    }

    fn polyhats(&mut self, rule: &Rule, position: usize, size: usize, kind: &'static str, stats: &mut RuleStats, tally: &mut Tally) -> bool {
        let root = self.position_cells[position];
        let area = ball(&mut self.world, root, size as u32 + 1);
        let mut adjacent: HashMap<u32, Vec<u32>> = HashMap::with_capacity(area.len());
        for &cell in &area {
            let mut around: Vec<u32> = self.world.touching(cell).to_vec();
            around.sort_by_key(|&c| self.world.key_of(c));
            adjacent.insert(cell, around);
        }
        let origin = Origin::plain(kind, Some(position), self.config.positions[position].class);
        let end = self.family_end;
        connected_sets(&adjacent, root, size, &mut |set| {
            if stats.seeds >= end {
                return false;
            }
            if kind == "late" {
                self.evaluate_late(rule, &origin, set, true, stats, tally);
            } else {
                self.evaluate(rule, &origin, set, 16, true, None, stats, tally);
            }
            true
        })
    }

    /// Chains running along a corridor: each step touches the last and gets further.
    fn corridor_seeds(&mut self, rule: &Rule, index: usize, stats: &mut RuleStats, tally: &mut Tally) {
        let (cells, starts) = self.corridor_cells[index].clone();
        let inside: HashSet<u32> = cells.iter().copied().collect();
        let (s, c) = self.config.corridors[index].axis.to_radians().sin_cos();
        let along = |w: &HatWorld, cell: u32| {
            let p = w.center(cell);
            p.0 * c + p.1 * s
        };
        let mut chains: Vec<Vec<u32>> = Vec::new();
        let mut seen: HashSet<Vec<u32>> = HashSet::new();
        for &start in &starts {
            for sign in [1.0, -1.0] {
                let mut stack = vec![vec![start]];
                while let Some(path) = stack.pop() {
                    if path.len() >= 2 && chains.len() < CORRIDOR_MAX_PATHS {
                        let mut set = path.clone();
                        set.sort_unstable();
                        if seen.insert(set) {
                            chains.push(path.clone());
                        }
                    }
                    if path.len() == 6 {
                        continue;
                    }
                    let last = *path.last().unwrap();
                    let mut next: Vec<u32> = self
                        .world
                        .touching(last)
                        .iter()
                        .copied()
                        .filter(|n| inside.contains(n) && !path.contains(n) && (along(&self.world, *n) - along(&self.world, last)) * sign > 0.5)
                        .collect();
                    next.sort_by_key(|&n| self.world.key_of(n));
                    for n in next {
                        let mut longer = path.clone();
                        longer.push(n);
                        stack.push(longer);
                    }
                }
            }
        }
        chains.sort_by_key(|chain| (chain.len(), keys_of(&self.world, chain)));
        let origin = Origin::plain("corridor", Some(index), "corridor");
        for chain in chains {
            if stats.seeds >= self.family_end {
                stats.truncated = true;
                return;
            }
            self.evaluate(rule, &origin, &chain, 16, true, None, stats, tally);
        }
    }

    /// Adds, removes and moves cells of the patterns that got furthest, as long as
    /// that gets them further still at a bounded population.
    fn mutate(&mut self, rule: &Rule, rule_seed: u64, stats: &mut RuleStats, tally: &mut Tally) {
        let mut tops = std::mem::take(&mut tally.tops);
        tops.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| keys_of(&self.world, &a.1).cmp(&keys_of(&self.world, &b.1))));
        tops.truncate(self.config.mutate_top);
        for (index, (reach, cells, origin)) in tops.into_iter().enumerate() {
            let seed = mix_seed(&[rule_seed, 3, index as u64]) as u32;
            let mut rng = Rng::new(seed as u64);
            let (mut best, mut best_reach) = (cells, reach);
            for _ in 0..self.config.mutate_trials {
                let mut trial = best.clone();
                let pick = trial[rng.below(trial.len())];
                match rng.below(3) {
                    0 if trial.len() > 1 => trial.retain(|&c| c != pick),
                    choice => {
                        if choice == 2 && trial.len() > 1 {
                            trial.retain(|&c| c != pick);
                        }
                        let mut around: Vec<u32> = ball(&mut self.world, pick, 1).into_iter().filter(|c| !trial.contains(c)).collect();
                        if around.is_empty() {
                            continue;
                        }
                        around.sort_by_key(|&c| self.world.key_of(c));
                        trial.push(around[rng.below(around.len())]);
                    }
                }
                stats.seeds += 1;
                for &c in &trial {
                    self.world.set_alive(c);
                }
                let limits = self.limits(trial.len() as u32, 16);
                let result = run(&mut self.world, &limits, None, &mut self.trail);
                self.world.reset_when_ruled();
                if result.fate != Fate::Overpopulated && result.reach > best_reach {
                    best = trial;
                    best_reach = result.reach;
                }
            }
            if best_reach > reach {
                stats.mutations_improved += 1;
                stats.mutation_reach_units = stats.mutation_reach_units.max(best_reach);
                let mut mutated = Origin { kind: "mutation", mutation_seed: Some(seed), ..origin };
                mutated.soup_seed = None;
                self.evaluate(rule, &mutated, &best, 16, false, None, stats, tally);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate(&mut self, rule: &Rule, origin: &Origin, cells: &[u32], transient: u32, use_memo: bool, parent: Option<(&[u32], u32)>, stats: &mut RuleStats, tally: &mut Tally) {
        stats.seeds += 1;
        for &c in cells {
            self.world.set_alive(c);
        }
        let start_pop = cells.len() as u32;
        let limits = self.limits(start_pop, transient);
        let memo = use_memo.then_some(&self.memo);
        let result = run(&mut self.world, &limits, memo, &mut self.trail);
        if use_memo {
            if self.memo.len() > MEMO_LIMIT {
                self.memo.clear();
            }
            self.memo.extend(self.trail.keys().copied());
        }
        self.world.reset_when_ruled();
        self.judge(rule, origin, cells, &result, true, transient, parent, stats, tally);
    }

    /// Gliders often come out of a long, large and chaotic phase, which the limits
    /// for a flying pattern would stop right away. So here a seed may burn for long
    /// at many cells, and only what has left it faces the strict test, on its own.
    fn evaluate_late(&mut self, rule: &Rule, origin: &Origin, cells: &[u32], use_memo: bool, stats: &mut RuleStats, tally: &mut Tally) {
        stats.seeds += 1;
        for &c in cells {
            self.world.set_alive(c);
        }
        let limits = self.late_limits();
        let memo = use_memo.then_some(&self.memo);
        let result = run(&mut self.world, &limits, memo, &mut self.trail);
        if use_memo {
            if self.memo.len() > MEMO_LIMIT {
                self.memo.clear();
            }
            self.memo.extend(self.trail.keys().copied());
        }
        self.world.reset_when_ruled();
        stats.late_generations_longest = stats.late_generations_longest.max(result.generations);
        // Only a pattern small all along can be the traveller itself.
        let small = result.max_pop <= self.config.pop_cap(cells.len() as u32);
        self.judge(rule, origin, cells, &result, small, 16, None, stats, tally);
    }

    /// Files what a run has shown. The field has been cleared again by now.
    #[allow(clippy::too_many_arguments)]
    fn judge(&mut self, rule: &Rule, origin: &Origin, cells: &[u32], result: &Run, may_travel: bool, transient: u32, parent: Option<(&[u32], u32)>, stats: &mut RuleStats, tally: &mut Tally) {
        match result.fate {
            Fate::Died => stats.died += 1,
            Fate::Still => stats.still += 1,
            Fate::Oscillator(_) => stats.oscillators += 1,
            Fate::Overpopulated => stats.overpopulated += 1,
            Fate::Merged => stats.merged += 1,
            Fate::Stalled => stats.stalled += 1,
            _ => stats.out_of_generations += 1,
        }
        stats.best_reach_units = stats.best_reach_units.max(result.reach);

        if parent.is_none() {
            if result.fate != Fate::Overpopulated && result.reach >= 2.0 * UNITS_PER_RING_FAR {
                tally.tops.push((result.reach, cells.to_vec(), origin.clone()));
                if tally.tops.len() > 4 * self.config.mutate_top.max(1) {
                    tally.tops.sort_by(|a, b| b.0.total_cmp(&a.0));
                    tally.tops.truncate(self.config.mutate_top.max(1));
                }
            }
            if let Fate::Still | Fate::Oscillator(_) = result.fate {
                self.settled(origin, cells, result, tally);
            }
        }

        let gate = TRAVELLER_RINGS * UNITS_PER_RING_MIN;
        let bounded = !matches!(result.fate, Fate::Overpopulated | Fate::Capped);
        if may_travel && bounded && result.reach >= gate && result.region_stable() {
            self.travelled(rule, origin, cells, result, parent, tally);
        } else if parent.is_none() && result.spread >= DEPARTED && !result.last.is_empty() {
            self.cut_out(rule, origin, cells, result, transient, stats, tally);
        }
    }

    /// Looks for what has left a larger run behind, as a glider that shed some
    /// debris fails every test on the whole field. Pieces far from the start are
    /// tried again on their own, under the same strict test as everything else.
    #[allow(clippy::too_many_arguments)]
    fn cut_out(&mut self, rule: &Rule, origin: &Origin, cells: &[u32], result: &Run, _transient: u32, stats: &mut RuleStats, tally: &mut Tally) {
        let live = &result.last;
        let points: Vec<(f64, f64)> = live.iter().map(|&c| self.world.center(c)).collect();
        let mut group: Vec<usize> = (0..live.len()).collect();
        fn find(group: &mut [usize], i: usize) -> usize {
            let mut at = i;
            while group[at] != at {
                group[at] = group[group[at]];
                at = group[at];
            }
            at
        }
        for i in 0..live.len() {
            for j in i + 1..live.len() {
                if (points[i].0 - points[j].0).hypot(points[i].1 - points[j].1) <= CLUSTER_GAP {
                    let (a, b) = (find(&mut group, i), find(&mut group, j));
                    group[a] = b;
                }
            }
        }
        let mut clusters: HashMap<usize, Vec<u32>> = HashMap::new();
        for i in 0..live.len() {
            let root = find(&mut group, i);
            clusters.entry(root).or_default().push(live[i]);
        }
        let mut pieces: Vec<Vec<u32>> = clusters.into_values().collect();
        pieces.retain(|piece| {
            let p = centroid(&self.world, piece);
            (p.0 - result.start.0).hypot(p.1 - result.start.1) >= CLUSTER_MIN_DISTANCE && piece.len() as u32 <= self.config.max_pop
        });
        // Furthest first, as those are the ones that have really left.
        let far = |world: &HatWorld, piece: &[u32]| {
            let p = centroid(world, piece);
            (p.0 - result.start.0).hypot(p.1 - result.start.1)
        };
        pieces.sort_by(|a, b| far(&self.world, b).total_cmp(&far(&self.world, a)).then_with(|| keys_of(&self.world, a).cmp(&keys_of(&self.world, b))));
        pieces.truncate(MAX_CLUSTERS);
        for piece in pieces {
            stats.cuts_tried += 1;
            self.evaluate(rule, origin, &piece, 16, false, Some((cells, result.generations)), stats, tally);
        }
    }

    fn settled(&mut self, origin: &Origin, cells: &[u32], result: &Run, tally: &mut Tally) {
        let (category, period) = match result.fate {
            Fate::Oscillator(period) => ("oscillator", period),
            _ => ("still-life", 1),
        };
        let pop = result.last.len() as u32;
        let key = format!("{category}/{period}/{pop}");
        if let Some(&at) = tally.finds.get(&key) {
            tally.list[at].occurrences += 1;
            return;
        }
        tally.finds.insert(key, tally.list.len());
        let find = self.find(category, origin, cells, None, result, result.reach / UNITS_PER_RING_FAR, period, Vec::new());
        tally.list.push(find);
    }

    #[allow(clippy::too_many_arguments)]
    fn find(&self, category: &'static str, origin: &Origin, cells: &[u32], parent: Option<(&[u32], u32)>, result: &Run, rings: f64, period: u32, patches: Vec<PatchCheck>) -> Find {
        Find {
            category,
            kind: origin.kind,
            cells: keys_of(&self.world, cells),
            derived_from: parent.map(|(p, _)| keys_of(&self.world, p)),
            derived_generation: parent.map_or(0, |(_, g)| g),
            position: origin.position,
            position_class: origin.position_class,
            soup_seed: origin.soup_seed,
            soup_rings: origin.soup_rings,
            soup_density: origin.soup_density,
            mutation_seed: origin.mutation_seed,
            reach_rings: rings,
            reach_units: result.reach,
            generations: result.generations,
            angle: result.angle,
            min_pop: result.min_pop,
            max_pop: result.max_pop,
            period,
            occurrences: 1,
            patches,
            center: result.start,
        }
    }

    fn travelled(&mut self, rule: &Rule, origin: &Origin, cells: &[u32], result: &Run, parent: Option<(&[u32], u32)>, tally: &mut Tally) {
        let Some(rings) = rings_between(&mut self.world, result.start_cell, result.reach_cell) else { return };
        if (rings as f64) < TRAVELLER_RINGS {
            return;
        }
        let mut rings = rings as f64;
        let mut category = "traveller";
        let mut patches = Vec::new();
        if rings >= GLIDER_RINGS {
            let keys = keys_of(&self.world, cells);
            let limits = self.limits(cells.len() as u32, 16);
            patches = {
                let _only_one_at_a_time = self.verify_lock.lock().unwrap_or_else(|e| e.into_inner());
                verify_in_patches(self.config.hood, |w| {
                    w.reset_when_ruled();
                    w.set_masks_when_empty(rule.birth, rule.survive, self.config.hood.radius());
                    for &key in &keys {
                        let cell = w.cell_of(key).expect("seed exists");
                        w.set_alive(cell);
                    }
                }, &limits)
            };
            if patches.iter().all(|p| p.passed) {
                category = "glider-candidate";
                rings = patches.last().map_or(rings, |p| p.reached as f64);
            }
        }
        // The same pattern shows up from many seeds, and in six directions.
        let key = format!("{category}/{}", (result.angle / 5.0).round() as i64);
        let find = self.find(category, origin, cells, parent, result, rings, 0, patches);
        match tally.finds.get(&key) {
            Some(&at) => {
                let occurrences = tally.list[at].occurrences + 1;
                if find.reach_rings > tally.list[at].reach_rings {
                    tally.list[at] = find;
                }
                tally.list[at].occurrences = occurrences;
            }
            None => {
                tally.finds.insert(key, tally.list.len());
                tally.list.push(find);
            }
        }
    }

    pub fn world(&mut self) -> &mut HatWorld {
        &mut self.world
    }
}

/// Flies a pattern across patches of growing radius, on a field generated in full
/// beforehand, and stops before it can touch their edge. `setup` places the pattern.
pub fn verify_in_patches(hood: Hood, mut setup: impl FnMut(&mut HatWorld), limits: &Limits) -> Vec<PatchCheck> {
    let mut world = new_world(hood);
    let mut checks = Vec::new();
    setup(&mut world);
    let start = centroid(&world, world.live());
    let Some(start_cell) = nearest_to(&world, world.live(), start) else { return checks };
    let mut generation = 0u32;
    let mut peak_extent = 0.0f64;
    for &patch in &PATCH_RINGS {
        // The disc holds every shortest path to a cell of the patch.
        world.load_around(start.0, start.1, patch as f64 * UNITS_PER_RING_MAX + 40.0);
        let mut rings = vec![u32::MAX; world.cell_count()];
        rings[start_cell as usize] = 0;
        let mut frontier = vec![start_cell];
        let mut depth = 0;
        while !frontier.is_empty() && depth <= patch + 4 {
            depth += 1;
            let mut next = Vec::new();
            for &c in &frontier {
                for &n in world.touching(c) {
                    if rings[n as usize] == u32::MAX {
                        rings[n as usize] = depth;
                        next.push(n);
                    }
                }
            }
            frontier = next;
        }
        let margin = hood.radius() as u32 + 2;
        let mut check = PatchCheck { rings: patch, passed: false, reached: 0, generations: generation, note: "" };
        let mut stale = 0u32;
        loop {
            if world.population == 0 {
                check.note = "died";
                break;
            }
            let center = centroid(&world, world.live());
            let at = nearest_to(&world, world.live(), center).unwrap_or(start_cell);
            let edge = world.live().iter().any(|&c| rings.get(c as usize).is_none_or(|&r| r >= patch - margin));
            let here = rings.get(at as usize).copied().unwrap_or(u32::MAX);
            if edge {
                // Stopped short of the edge, with the centre about a pattern's width behind.
                check.reached = if here == u32::MAX { check.reached } else { here.max(check.reached) };
                check.passed = true;
                break;
            }
            if here != u32::MAX && here > check.reached {
                check.reached = here;
                stale = 0;
            } else {
                stale += 1;
                if stale > limits.patience * 4 {
                    check.note = "stopped travelling";
                    break;
                }
            }
            world.step();
            generation += 1;
            check.generations = generation;
            if world.population as u32 > if generation <= limits.transient { limits.transient_cap } else { limits.pop_cap } {
                check.note = "too many cells";
                break;
            }
            if generation > limits.transient {
                let center = centroid(&world, world.live());
                let extent = world.live().iter().map(|&c| {
                    let p = world.center(c);
                    (p.0 - center.0).hypot(p.1 - center.1)
                }).fold(0.0, f64::max);
                if peak_extent > 0.0 && generation > limits.transient + 64 && extent > 1.25 * peak_extent + 6.0 {
                    check.note = "region grew";
                    break;
                }
                peak_extent = peak_extent.max(extent);
            }
        }
        let passed = check.passed;
        checks.push(check);
        if !passed {
            break;
        }
    }
    checks
}

/// James Edmond's glider A, with four states. It is the one pattern known to travel,
/// so it tests the whole measurement, even though the search itself has only two.
pub const GLIDER_A_TABLE: [u32; 42] = [
    0, 1, 2, 2, 0, 255, 2, 1, 3, 1, 1, 3, 2, 2, 1, 1, 0, 1, 1, 1, 2, 2, 1, 2, 1, 3, 2, 0, 0, 0, 1, 1, 1, 2, 2, 1, 3, 2, 0, 2, 0, 0,
];

pub fn place_glider_a(world: &mut HatWorld, cell: u32) {
    world.scratch = GLIDER_A_TABLE.to_vec();
    world.set_table_from_scratch(GLIDER_A_TABLE.len());
    world.seed_pair(cell, 1, 2);
}

/// What the positive control needs: the search's limits for a 2-cell seed.
pub fn control_limits(config: &Config) -> Limits {
    Limits {
        pop_cap: config.pop_cap(2),
        transient: 16,
        transient_cap: 100,
        patience: config.patience,
        max_generations: config.max_generations,
        stop_reach: GLIDER_RINGS * UNITS_PER_RING_MAX,
        stop_spread: f64::INFINITY,
    }
}

pub fn control_world() -> HatWorld {
    new_world(Hood::Moore)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brute_force(adjacent: &HashMap<u32, Vec<u32>>, root: u32, size: usize) -> HashSet<Vec<u32>> {
        let cells: Vec<u32> = adjacent.keys().copied().collect();
        let mut found = HashSet::new();
        for mask in 0u32..1 << cells.len() {
            if mask.count_ones() as usize != size {
                continue;
            }
            let set: Vec<u32> = (0..cells.len()).filter(|i| mask >> i & 1 == 1).map(|i| cells[i]).collect();
            if !set.contains(&root) {
                continue;
            }
            let mut reached = vec![root];
            let mut i = 0;
            while i < reached.len() {
                for n in &adjacent[&reached[i]] {
                    if set.contains(n) && !reached.contains(n) {
                        reached.push(*n);
                    }
                }
                i += 1;
            }
            if reached.len() == size {
                let mut sorted = set.clone();
                sorted.sort_unstable();
                found.insert(sorted);
            }
        }
        found
    }

    #[test]
    fn connected_sets_finds_each_set_once() {
        let mut world = HatWorld::new();
        let root = world.cell_of(world.key_of(50)).unwrap();
        let area = ball(&mut world, root, 2);
        let inside: HashSet<u32> = area.iter().copied().collect();
        let adjacent: HashMap<u32, Vec<u32>> = area.iter().map(|&c| (c, world.touching(c).iter().copied().filter(|n| inside.contains(n)).collect())).collect();
        assert!(area.len() <= 24, "{} cells are too many for brute force", area.len());
        for size in 2..=5 {
            let expected = brute_force(&adjacent, root, size);
            let mut found = Vec::new();
            connected_sets(&adjacent, root, size, &mut |set| {
                let mut sorted = set.to_vec();
                sorted.sort_unstable();
                found.push(sorted);
                true
            });
            assert_eq!(found.len(), expected.len(), "size {size}: duplicates or gaps");
            assert_eq!(found.into_iter().collect::<HashSet<_>>(), expected, "size {size}");
        }
    }

    fn config(hood: Hood) -> Config {
        let mut world = HatWorld::new();
        let positions = select_positions(&mut world, 5);
        Config {
            hood,
            plan: SeedPlan { polyhat: Some((3, 3)), corridor: false, soup: Some((3, 3)), mutate: true, late: None, late_soup: None },
            positions,
            corridors: Vec::new(),
            master_seed: 1,
            pop_factor: 3,
            pop_floor: 24,
            max_pop: 60,
            patience: 200,
            max_generations: 4000,
            screen_soups: 3,
            screen_generations: 200,
            screen_rings: 6,
            keep: vec![RuleClass::Stable, RuleClass::Chaotic],
            soups_each: 1,
            densities: vec![0.5],
            max_seeds_per_rule: 1000,
            mutate_top: 1,
            mutate_trials: 5,
            late_generations: 2000,
            late_max_pop: 300,
        }
    }

    #[test]
    fn rule_names_round_trip() {
        for name in ["B36/S234", "B/S", "B7/S01234567", "B1/S"] {
            assert_eq!(Rule::parse(name, Hood::Moore).unwrap().name, name);
        }
        assert_eq!(Rule::parse("R2B5-7S3-8", Hood::Radius(2)).unwrap().name, "R2B5-7S3-8");
        assert!(Rule::parse("B03/S23", Hood::Moore).is_err());
        assert!(Rule::parse("B8/S23", Hood::Moore).is_err());
        assert!(Rule::parse("R2B0-7S3-8", Hood::Radius(2)).is_err());
        assert!(Rule::parse("R3B1-7S3-8", Hood::Radius(2)).is_err());
    }

    #[test]
    fn the_rule_space_has_the_expected_size() {
        assert_eq!(Rule::all(Hood::Moore).len(), 32_768);
        assert_eq!(Rule::all(Hood::Edge).len(), 8_192);
        assert_eq!(Rule::all(Hood::Radius(2)).len(), 69_828);
    }

    #[test]
    fn positions_cover_all_classes_and_exist_anywhere() {
        let mut world = HatWorld::new();
        let positions = select_positions(&mut world, 20);
        assert_eq!(positions.len(), 20);
        for class in POSITION_CLASSES {
            assert_eq!(positions.iter().filter(|p| p.class == class).count(), 4, "{class}");
        }
        let mut other = HatWorld::new();
        for p in &positions {
            let cell = other.cell_of(p.key).unwrap();
            assert_eq!(p.class, POSITION_CLASSES[class_of(&other, cell)]);
        }
    }

    #[test]
    fn results_do_not_depend_on_the_history_of_the_world() {
        let lock = Mutex::new(());
        let c = config(Hood::Moore);
        let rule = Rule::parse("B25/S3", Hood::Moore).unwrap();
        let summary = |r: &RuleReport| -> Vec<String> { r.finds.iter().map(|f| format!("{} {:?} {} {}", f.category, f.cells, f.max_pop, f.generations)).collect() };
        let fresh = Searcher::new(&c, &lock).search_rule(&rule);
        let mut used = Searcher::new(&c, &lock);
        used.world().load_around(200.0, 100.0, 60.0);
        used.search_rule(&Rule::parse("B36/S234", Hood::Moore).unwrap());
        let later = used.search_rule(&rule);
        assert_eq!(fresh.class, later.class);
        assert_eq!(summary(&fresh), summary(&later));
        assert_eq!(fresh.stats.seeds, later.stats.seeds);
    }

    /// The search has to recognise the one glider there is, at its full distance:
    /// a flight that stops short of it along a fast axis would silently find nothing.
    #[test]
    fn glider_a_flies_far_enough_to_count() {
        let c = config(Hood::Moore);
        let limits = control_limits(&c);
        let mut world = control_world();
        place_glider_a(&mut world, 17);
        let mut trail = HashMap::new();
        let result = run(&mut world, &limits, None, &mut trail);
        assert_eq!(result.fate, Fate::ReachedTarget);
        assert!(result.region_stable());
        assert!(result.max_pop <= limits.pop_cap);
        let rings = rings_between(&mut world, result.start_cell, result.reach_cell).unwrap();
        assert!(rings as f64 >= GLIDER_RINGS, "only {rings} rings");
    }

    #[test]
    fn glider_a_passes_a_patch() {
        let c = config(Hood::Moore);
        let limits = control_limits(&c);
        let mut world = control_world();
        place_glider_a(&mut world, 17);
        let mut trail = HashMap::new();
        let flight = run(&mut world, &Limits { stop_reach: 110.0 * UNITS_PER_RING_MAX, ..limits }, None, &mut trail);
        assert!(flight.region_stable());
        let checks = verify_in_patches(Hood::Moore, |w| place_glider_a(w, 17), &limits);
        assert!(checks[0].passed, "{:?}", checks[0]);
    }

    /// All the way through the judgement that decides what gets reported, as what
    /// would otherwise be tested is only that no find is ever reported.
    #[test]
    fn a_flight_is_reported_as_a_traveller() {
        let lock = Mutex::new(());
        let c = config(Hood::Moore);
        let mut searcher = Searcher::new(&c, &lock);
        let limits = Limits { stop_reach: 45.0 * UNITS_PER_RING_MAX, ..control_limits(&c) };
        let mut trail = HashMap::new();
        // Some starts send off two gliders, which is rightly no single travelling pattern.
        let starts = searcher.position_cells.clone();
        let (cells, result) = starts
            .iter()
            .find_map(|&start| {
                place_glider_a(searcher.world(), start);
                let cells: Vec<u32> = searcher.world().live().to_vec();
                let result = run(searcher.world(), &limits, None, &mut trail);
                searcher.world().reset_when_ruled();
                (result.fate == Fate::ReachedTarget && result.region_stable()).then_some((cells, result))
            })
            .expect("a start with one glider");
        let (mut stats, mut tally) = (RuleStats::default(), Tally { finds: HashMap::new(), list: Vec::new(), tops: Vec::new() });
        let origin = Origin::plain("polyhat", Some(0), "H");
        let rule = Rule::bits(0, 0);
        searcher.judge(&rule, &origin, &cells, &result, true, 16, None, &mut stats, &mut tally);
        assert_eq!(tally.list.len(), 1, "{result:?}");
        let find = &tally.list[0];
        assert_eq!(find.category, "traveller");
        assert!(find.reach_rings >= TRAVELLER_RINGS, "{} rings", find.reach_rings);
        assert_eq!(find.cells.len(), 2);
    }

    /// Two gliders flying apart are the simplest thing that leaves a seed behind: under
    /// the late limits the run stops once they are out, and both get cut out.
    #[test]
    fn what_leaves_a_late_run_is_cut_out_and_judged_alone() {
        let lock = Mutex::new(());
        let mut c = config(Hood::Moore);
        let mut world = HatWorld::new();
        c.positions = select_positions(&mut world, 20);
        let mut searcher = Searcher::new(&c, &lock);
        let limits = searcher.late_limits();
        let mut trail = HashMap::new();
        let starts = searcher.position_cells.clone();
        let (cells, result) = starts
            .iter()
            .find_map(|&start| {
                place_glider_a(searcher.world(), start);
                let cells: Vec<u32> = searcher.world().live().to_vec();
                let result = run(searcher.world(), &limits, None, &mut trail);
                searcher.world().reset_when_ruled();
                (result.fate == Fate::ReachedTarget && result.reach < 100.0).then_some((cells, result))
            })
            .expect("a start with two gliders");
        assert!(result.spread >= LATE_STOP_SPREAD);
        assert!(result.generations < limits.max_generations);
        let (mut stats, mut tally) = (RuleStats::default(), Tally { finds: HashMap::new(), list: Vec::new(), tops: Vec::new() });
        let origin = Origin::plain("late", Some(0), "H");
        // Without the states of the table, the pieces cannot fly on; this is about the cutting.
        searcher.world().set_masks_when_empty(0, 0, 1);
        searcher.judge(&Rule::bits(0, 0), &origin, &cells, &result, false, 16, None, &mut stats, &mut tally);
        assert_eq!(stats.cuts_tried, 2, "{result:?}");
    }

    #[test]
    fn a_large_late_run_cannot_pass_as_a_traveller() {
        let lock = Mutex::new(());
        let c = config(Hood::Moore);
        let mut searcher = Searcher::new(&c, &lock);
        let start = searcher.position_cells[0];
        place_glider_a(searcher.world(), start);
        let cells: Vec<u32> = searcher.world().live().to_vec();
        let limits = Limits { stop_reach: 45.0 * UNITS_PER_RING_MAX, ..control_limits(&c) };
        let mut trail = HashMap::new();
        let result = run(searcher.world(), &limits, None, &mut trail);
        searcher.world().reset_when_ruled();
        let (mut stats, mut tally) = (RuleStats::default(), Tally { finds: HashMap::new(), list: Vec::new(), tops: Vec::new() });
        searcher.world().set_masks_when_empty(0, 0, 1);
        searcher.judge(&Rule::bits(0, 0), &Origin::plain("late", Some(0), "H"), &cells, &result, false, 16, None, &mut stats, &mut tally);
        assert!(tally.list.iter().all(|f| f.category != "traveller" && f.category != "glider-candidate"));
    }

    #[test]
    fn late_seeds_stop_at_their_limits() {
        let lock = Mutex::new(());
        let mut c = config(Hood::Moore);
        c.plan = SeedPlan { polyhat: None, corridor: false, soup: None, mutate: false, late: Some((3, 4)), late_soup: Some((3, 3)) };
        c.keep = vec![RuleClass::Explosive, RuleClass::Stable, RuleClass::Chaotic];
        c.late_generations = 300;
        c.late_max_pop = 50;
        let mut searcher = Searcher::new(&c, &lock);
        for name in ["B27/S346", "B2/S", "B3/S23"] {
            let report = searcher.search_rule(&Rule::parse(name, Hood::Moore).unwrap());
            assert!(report.searched && report.stats.seeds > 0, "{name}");
            assert!(report.stats.late_generations_longest <= 300, "{name}");
            for find in &report.finds {
                assert_eq!(find.kind, "late");
                assert!(find.category == "still-life" || find.category == "oscillator" || find.max_pop <= 60, "{find:?}");
            }
        }
    }

    #[test]
    fn one_family_of_seeds_does_not_use_up_the_budget_of_the_next() {
        let lock = Mutex::new(());
        let mut c = config(Hood::Moore);
        let mut world = HatWorld::new();
        c.corridors = find_corridors(&mut world, 2).0;
        c.plan = SeedPlan { polyhat: Some((3, 6)), corridor: true, soup: Some((3, 3)), mutate: false, late: None, late_soup: None };
        c.max_seeds_per_rule = 50;
        let rule = Rule::parse("B27/S346", Hood::Moore).unwrap();
        let report = Searcher::new(&c, &lock).search_rule(&rule);
        assert!(report.searched);
        assert!(report.stats.truncated);
        assert!(report.stats.seeds > 2 * 50, "{} seeds", report.stats.seeds);
    }

    #[test]
    fn corridors_lie_between_lines_of_mirrored_hats() {
        let mut world = HatWorld::new();
        let (corridors, stats) = find_corridors(&mut world, 10);
        assert_eq!(corridors.len(), 10);
        assert!(stats.lines.iter().any(|&hats| hats >= 20));
        for corridor in &corridors {
            assert!(!corridor.starts.is_empty());
            let mirrored = corridor.cells.iter().filter(|k| k[0] >= 8).count();
            assert!(mirrored >= 2, "{mirrored} mirrored hats on the walls of {corridor:?}");
        }
    }

    #[test]
    fn a_rule_without_birth_dies_or_stands() {
        let lock = Mutex::new(());
        let c = config(Hood::Moore);
        let report = Searcher::new(&c, &lock).search_rule(&Rule::bits(0, 0));
        assert_eq!(report.class, RuleClass::Dies);
        assert!(!report.searched);
    }

    #[test]
    fn range_rules_run_in_the_search() {
        let lock = Mutex::new(());
        let c = config(Hood::Radius(2));
        let report = Searcher::new(&c, &lock).search_rule(&Rule::parse("R2B5-7S3-8", Hood::Radius(2)).unwrap());
        assert!(report.screens.iter().all(|s| s.start_pop > 0));
    }
}
