//! Recognises when a pattern has stopped doing anything new.
//!
//! A periodic population is necessary for every settled state, and cheap to
//! track, so it decides when the expensive checks run. It proves nothing on its
//! own: a glider about to hit a block has a constant population too.
//!
//! - `Still` and `Empty` follow directly from no chunk having changed.
//! - `Oscillating` means the whole field's hash recurred.
//! - `Drifting` means the field splits into objects that repeat up to a
//!   translation, and that objects of different velocity can never meet again.
//!   This is a proof rather than a guess, see `drift`.
//! - `Growing` is a heuristic based on the population alone.
//!
//! On a torus, whatever moves comes back, so there is no drift: gliders end up as
//! one long oscillation of the whole field. Its period can be huge, so the hash
//! comparison keeps going there with a growing distance, see `advance`.

use crate::life::{SIZE, Topology, World};
use std::collections::HashMap;

/// What the detector needs to know about a field.
pub trait Observed {
    fn generation(&self) -> u64;
    fn population(&self) -> u64;
    fn any_changed(&self) -> bool;
    fn field_hash(&self) -> u64;
    /// Splits the field into objects, if moving ones can be proven to repeat. That
    /// needs a translation mapping the field to itself. An aperiodic tiling has
    /// none: gliders exist there, but they re-form on tiles they have never visited.
    fn objects(&self) -> Option<(Vec<Object>, Topology)>;
    /// False where a rule can bring an empty field back to life.
    fn empty_is_final(&self) -> bool {
        true
    }
    /// A finite field has to repeat eventually, so looking on is never in vain.
    fn period_is_unbounded(&self) -> bool {
        false
    }
}

const HISTORY: usize = 4096;
const MAX_PERIOD: u32 = 720;
const MAX_TORUS_PERIOD: u32 = 1 << 30;
const SCAN_INTERVAL: u64 = 32;
const RETRY_AFTER: u64 = 256;
/// Period candidates for the object comparison are divisors of this number.
const HIGHLY_COMPOSITE: u32 = 5040;
const MAX_POPULATION: u64 = 2_000_000;
const MAX_PAIR_CHECKS: u64 = 50_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Status {
    #[default]
    Active,
    Empty,
    Still,
    Oscillating {
        period: u32,
    },
    Drifting {
        period: u32,
        moving: u32,
    },
    Growing {
        period: u32,
        delta: u32,
    },
}

impl Status {
    /// Settled states are permanent until the field is edited, because the
    /// automaton is deterministic. `Growing` is only a current observation.
    fn settled(self) -> bool {
        !matches!(self, Status::Active | Status::Growing { .. })
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Shape {
    hash: u64,
    population: u32,
    width: i32,
    height: i32,
}

#[derive(Clone, Copy)]
pub struct Object {
    shape: Shape,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

struct Analysis {
    start: u64,
    population: u64,
    hash: u64,
    objects: Vec<Object>,
    population_period: u32,
    /// How far this hash is compared before giving up or starting over.
    limit: u32,
    /// Set while confirming a drift over a second period of this length.
    confirm: Option<u32>,
}

#[derive(Default)]
pub struct Detector {
    pub status: Status,
    /// Generation at which the current status was recognised.
    pub since: u64,
    history: Vec<u64>,
    head: usize,
    analysis: Option<Analysis>,
    next_scan: u64,
}

impl Detector {
    pub fn invalidate(&mut self) {
        self.status = Status::Active;
        self.since = 0;
        self.history.clear();
        self.head = 0;
        self.analysis = None;
        self.next_scan = 0;
    }

    fn push(&mut self, population: u64) {
        if self.history.len() < HISTORY {
            self.history.push(population);
        } else {
            self.history[self.head] = population;
            self.head = (self.head + 1) % HISTORY;
        }
    }

    /// Population `back` generations ago.
    fn past(&self, back: usize) -> u64 {
        let n = self.history.len();
        if n < HISTORY {
            self.history[n - 1 - back]
        } else {
            self.history[(self.head + HISTORY - 1 - back) % HISTORY]
        }
    }

    fn set(&mut self, status: Status, generation: u64) {
        if self.status != status {
            self.status = status;
            self.since = generation;
        }
    }

    pub(crate) fn observe(&mut self, world: &impl Observed) {
        if self.status.settled() {
            return;
        }
        let generation = world.generation();
        self.push(world.population());

        if world.population() == 0 && world.empty_is_final() {
            self.set(Status::Empty, generation);
        } else if !world.any_changed() {
            self.set(Status::Still, generation);
        } else if self.analysis.is_some() {
            self.advance(world);
        } else if generation >= self.next_scan && generation % SCAN_INTERVAL == 0 {
            self.scan(world);
        }
    }

    /// Finds the shortest period over which the population repeats, possibly
    /// with a constant gain.
    fn scan(&mut self, world: &impl Observed) {
        let n = self.history.len();
        let mut found = None;
        for period in 1..=MAX_PERIOD as usize {
            let window = 2 * period + 32;
            if n < window + period {
                break;
            }
            let delta = self.past(0) as i64 - self.past(period) as i64;
            let repeats =
                (1..window).all(|i| self.past(i) as i64 - self.past(i + period) as i64 == delta);
            if repeats {
                found = Some((period as u32, delta));
                break;
            }
        }
        let generation = world.generation();
        match found {
            Some((period, 0)) if world.population() <= MAX_POPULATION => {
                self.set(Status::Active, generation);
                self.analysis = Some(Analysis {
                    start: generation,
                    population: world.population(),
                    hash: world.field_hash(),
                    objects: world.objects().map_or(Vec::new(), |(objects, _)| objects),
                    population_period: period,
                    limit: MAX_PERIOD,
                    confirm: None,
                });
            }
            Some((period, delta)) if delta > 0 => {
                let status = Status::Growing {
                    period,
                    delta: delta as u32,
                };
                self.set(status, generation);
            }
            _ => self.set(Status::Active, generation),
        }
    }

    fn advance(&mut self, world: &impl Observed) {
        let generation = world.generation();
        let mut analysis = self.analysis.take().expect("checked by the caller");
        let k = (generation - analysis.start) as u32;

        // Once the population stops repeating, there is nothing left to find.
        if self.past(analysis.population_period as usize) != world.population() {
            self.next_scan = generation + SCAN_INTERVAL;
            return;
        }

        if world.population() == analysis.population {
            if analysis.confirm.is_none() && world.field_hash() == analysis.hash {
                self.set(Status::Oscillating { period: k }, generation);
                return;
            }
            let period = analysis.population_period;
            let candidate = match analysis.confirm {
                Some(expected) => expected == k,
                None => {
                    HIGHLY_COMPOSITE % k == 0 || (period > 1 && k % period == 0 && k / period <= 8)
                }
            };
            if candidate && let Some((objects, topology)) = world.objects() {
                if let Some(moving) = drift(&analysis.objects, &objects, k, topology) {
                    if analysis.confirm.is_some() {
                        self.set(Status::Drifting { period: k, moving }, generation);
                        return;
                    }
                    // A second round rules out a lucky match of identical shapes.
                    analysis.start = generation;
                    analysis.objects = objects;
                    analysis.confirm = Some(k);
                    self.analysis = Some(analysis);
                    return;
                }
            }
        }

        if k >= analysis.confirm.unwrap_or(analysis.limit) {
            // Brent's cycle detection: a new reference at twice the distance finds
            // any period, at no more than three times its length in comparisons.
            if world.period_is_unbounded() && analysis.limit < MAX_TORUS_PERIOD {
                analysis.start = generation;
                analysis.population = world.population();
                analysis.hash = world.field_hash();
                analysis.limit *= 2;
                self.analysis = Some(analysis);
                return;
            }
            self.next_scan = generation + RETRY_AFTER;
            return;
        }
        self.analysis = Some(analysis);
    }
}

/// Decides whether `after`, taken `period` generations past `before`, consists
/// of the same objects, and returns how many of them moved.
///
/// Influence spreads by at most `range` cells per generation. So if objects of
/// different velocity are further apart than `2 * range * (period + 1)` on some
/// axis, and not approaching on it, no object was disturbed during the observed
/// period, and none will be during any later one. Each velocity group therefore
/// repeats what it just did, indefinitely.
fn drift(before: &[Object], after: &[Object], period: u32, topology: Topology) -> Option<u32> {
    if before.len() != after.len() {
        return None;
    }
    let range = topology.range();
    let reach = period as i32 * range;

    let mut by_place: HashMap<(Shape, i32, i32), Vec<usize>> = HashMap::new();
    for (i, o) in after.iter().enumerate() {
        by_place.entry((o.shape, o.x0, o.y0)).or_default().push(i);
    }
    let mut taken = vec![false; after.len()];
    let mut pending = Vec::new();
    for (i, o) in before.iter().enumerate() {
        match by_place
            .get_mut(&(o.shape, o.x0, o.y0))
            .and_then(|v| v.pop())
        {
            Some(j) => taken[j] = true,
            None => pending.push(i),
        }
    }
    if pending.is_empty() {
        return None;
    }

    let mut moving = Vec::with_capacity(pending.len());
    for &i in &pending {
        let o = &before[i];
        let mut best: Option<(usize, i32)> = None;
        for (j, candidate) in after.iter().enumerate() {
            if taken[j] || candidate.shape != o.shape {
                continue;
            }
            let (dx, dy) = (candidate.x0 - o.x0, candidate.y0 - o.y0);
            if topology.parity_matters() && (dx + dy).rem_euclid(2) != 0 {
                continue;
            }
            let distance = dx.abs().max(dy.abs());
            if distance <= reach && best.is_none_or(|(_, d)| distance < d) {
                best = Some((j, distance));
            }
        }
        let (j, _) = best?;
        taken[j] = true;
        moving.push((i, (after[j].x0 - o.x0, after[j].y0 - o.y0)));
    }

    if moving.len() as u64 * before.len() as u64 > MAX_PAIR_CHECKS {
        return None;
    }
    let mut velocity = vec![(0, 0); before.len()];
    for &(i, v) in &moving {
        velocity[i] = v;
    }
    let margin = 2 * reach + 2 * range;
    for &(i, v) in &moving {
        for (j, other) in before.iter().enumerate() {
            if i != j && velocity[j] != v && !diverged(&before[i], v, other, velocity[j], margin) {
                return None;
            }
        }
    }
    Some(moving.len() as u32)
}

fn diverged(a: &Object, va: (i32, i32), b: &Object, vb: (i32, i32), margin: i32) -> bool {
    let apart = |a0: i32, a1: i32, va: i32, b0: i32, b1: i32, vb: i32| {
        (b0 - a1 > margin && vb >= va) || (a0 - b1 > margin && va >= vb)
    };
    apart(a.x0, a.x1, va.0, b.x0, b.x1, vb.0) || apart(a.y0, a.y1, va.1, b.y0, b.y1, vb.1)
}

fn mix(a: u64, b: u64) -> u64 {
    let x = (a ^ b.rotate_left(32)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^ (x >> 29)
}

impl Observed for World {
    fn generation(&self) -> u64 {
        self.generation
    }
    fn population(&self) -> u64 {
        self.population
    }
    fn any_changed(&self) -> bool {
        self.chunks.iter().any(|c| c.changed)
    }
    fn field_hash(&self) -> u64 {
        World::field_hash(self)
    }
    fn objects(&self) -> Option<(Vec<Object>, Topology)> {
        match self.torus {
            Some(_) => None,
            None => Some((World::objects(self), self.topology)),
        }
    }
    fn empty_is_final(&self) -> bool {
        self.birth & 1 == 0
    }
    fn period_is_unbounded(&self) -> bool {
        self.torus.is_some()
    }
}

impl World {
    pub(crate) fn observe(&mut self) {
        let mut detector = std::mem::take(&mut self.detector);
        detector.observe(self);
        self.detector = detector;
    }

    /// Independent of insertion order and of empty chunks.
    fn field_hash(&self) -> u64 {
        let mut total = 0u64;
        for c in self.chunks.iter().filter(|c| c.pop > 0) {
            let mut h = mix(c.key.0 as u32 as u64, c.key.1 as u32 as u64);
            for &row in c.rows() {
                h = mix(h, row);
            }
            total ^= h;
        }
        total
    }

    /// Splits the field into objects. Cells within twice the neighbourhood range
    /// can share a neighbour and thus influence one another, so they belong
    /// together.
    fn objects(&self) -> Vec<Object> {
        let mut remaining: HashMap<(i32, i32), [u64; SIZE]> = self
            .chunks
            .iter()
            .filter(|c| c.pop > 0)
            .map(|c| (c.key, *c.rows()))
            .collect();
        let keys: Vec<(i32, i32)> = remaining.keys().copied().collect();

        let take = |remaining: &mut HashMap<(i32, i32), [u64; SIZE]>, x: i32, y: i32| {
            let Some(rows) = remaining.get_mut(&(x >> 6, y >> 6)) else {
                return false;
            };
            let row = &mut rows[(y & 63) as usize];
            let bit = 1u64 << (x & 63);
            let live = *row & bit != 0;
            *row &= !bit;
            live
        };

        let radius = 2 * self.topology.range();
        let mut objects = Vec::new();
        let mut stack = Vec::new();
        let mut cells = Vec::new();
        for key in keys {
            for ry in 0..SIZE {
                loop {
                    let row = remaining[&key][ry];
                    if row == 0 {
                        break;
                    }
                    let x = key.0 * 64 + row.trailing_zeros() as i32;
                    let y = key.1 * 64 + ry as i32;
                    take(&mut remaining, x, y);
                    stack.push((x, y));
                    cells.clear();
                    while let Some((x, y)) = stack.pop() {
                        cells.push((x, y));
                        for dy in -radius..=radius {
                            for dx in -radius..=radius {
                                if take(&mut remaining, x + dx, y + dy) {
                                    stack.push((x + dx, y + dy));
                                }
                            }
                        }
                    }
                    objects.push(object_of(&cells));
                }
            }
        }
        objects
    }
}

fn object_of(cells: &[(i32, i32)]) -> Object {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for &(x, y) in cells {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    // Summed so that the order of the flood fill does not matter.
    let hash = cells.iter().fold(0u64, |sum, &(x, y)| {
        sum.wrapping_add(mix((x - x0) as u64 + 1, (y - y0) as u64 + 1))
    });
    Object {
        shape: Shape {
            hash,
            population: cells.len() as u32,
            width: x1 - x0,
            height: y1 - y0,
        },
        x0,
        y0,
        x1,
        y1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GLIDER: [(i32, i32); 5] = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    const BLINKER: [(i32, i32); 3] = [(0, 0), (1, 0), (2, 0)];
    const BLOCK: [(i32, i32); 4] = [(0, 0), (1, 0), (0, 1), (1, 1)];

    fn world(parts: &[(&[(i32, i32)], i32, i32)]) -> World {
        let mut w = World::new();
        for (cells, ox, oy) in parts {
            for (x, y) in cells.iter() {
                w.set(x + ox, y + oy, true);
            }
        }
        w
    }

    fn run_until_settled(w: &mut World, limit: u32) -> Status {
        for _ in 0..limit {
            w.step();
            if w.detector.status.settled() {
                break;
            }
        }
        w.detector.status
    }

    #[test]
    fn dying_pattern_is_empty() {
        let mut w = world(&[(&[(0, 0), (1, 0)], 0, 0)]);
        assert_eq!(run_until_settled(&mut w, 10), Status::Empty);
    }

    #[test]
    fn block_is_still() {
        let mut w = world(&[(&BLOCK, 0, 0)]);
        assert_eq!(run_until_settled(&mut w, 10), Status::Still);
    }

    #[test]
    fn blinker_oscillates_with_its_true_period() {
        let mut w = world(&[(&BLINKER, 0, 0)]);
        assert_eq!(
            run_until_settled(&mut w, 500),
            Status::Oscillating { period: 2 }
        );
    }

    #[test]
    fn mixed_periods_are_found_together() {
        // A period 3 pulsar and a period 2 blinker make 6.
        let pulsar_quarter = [(2, 0), (3, 0), (4, 0), (0, 2), (0, 3), (0, 4), (5, 2), (5, 3), (5, 4), (2, 5), (3, 5), (4, 5)];
        let pulsar: Vec<(i32, i32)> = pulsar_quarter
            .iter()
            .flat_map(|&(x, y)| [(x, y), (12 - x, y), (x, 12 - y), (12 - x, 12 - y)])
            .collect();
        let mut w = world(&[(&pulsar, 0, 0), (&BLINKER, 40, 40)]);
        assert_eq!(
            run_until_settled(&mut w, 1000),
            Status::Oscillating { period: 6 }
        );
    }

    #[test]
    fn lone_glider_drifts() {
        let mut w = world(&[(&GLIDER, 0, 0)]);
        assert_eq!(
            run_until_settled(&mut w, 2000),
            Status::Drifting {
                period: 4,
                moving: 1
            }
        );
    }

    #[test]
    fn glider_leaving_an_oscillator_drifts() {
        let mut w = world(&[(&GLIDER, 0, 0), (&BLINKER, -60, -60), (&BLOCK, -80, 0)]);
        assert_eq!(
            run_until_settled(&mut w, 4000),
            Status::Drifting {
                period: 4,
                moving: 1
            }
        );
    }

    #[test]
    fn glider_on_collision_course_is_not_settled_before_impact() {
        // The glider flies towards the block along the diagonal, keeping its
        // population constant all the way.
        let mut w = world(&[(&GLIDER, 0, 0), (&BLOCK, 300, 300)]);
        let impact = 4 * 290;
        for _ in 0..impact {
            w.step();
            assert!(
                !w.detector.status.settled(),
                "claimed {:?} at generation {}",
                w.detector.status,
                w.generation
            );
        }
    }

    #[test]
    fn gliders_flying_apart_drift() {
        let mirrored: Vec<(i32, i32)> = GLIDER.iter().map(|&(x, y)| (-x, -y)).collect();
        let mut w = world(&[(&GLIDER, 0, 0), (&mirrored, -10, -10)]);
        assert_eq!(
            run_until_settled(&mut w, 3000),
            Status::Drifting {
                period: 4,
                moving: 2
            }
        );
    }

    #[test]
    fn gosper_gun_grows() {
        let gun = [
            (24, 0), (22, 1), (24, 1), (12, 2), (13, 2), (20, 2), (21, 2), (34, 2), (35, 2),
            (11, 3), (15, 3), (20, 3), (21, 3), (34, 3), (35, 3), (0, 4), (1, 4), (10, 4),
            (16, 4), (20, 4), (21, 4), (0, 5), (1, 5), (10, 5), (14, 5), (16, 5), (17, 5),
            (22, 5), (24, 5), (10, 6), (16, 6), (24, 6), (11, 7), (15, 7), (12, 8), (13, 8),
        ];
        let mut w = world(&[(&gun, 0, 0)]);
        for _ in 0..1000 {
            w.step();
        }
        assert_eq!(
            w.detector.status,
            Status::Growing {
                period: 30,
                delta: 5
            }
        );
    }

    /// Whatever is claimed on any grid has to stay true far into the future.
    #[test]
    fn claims_hold_on_every_grid() {
        let bits = |counts: &[u32]| counts.iter().fold(0u16, |m, c| m | 1 << c);
        let cases = [
            (Topology::VonNeumann, bits(&[2, 4]), bits(&[2, 3])),
            (Topology::VonNeumann, bits(&[3]), bits(&[1, 2])),
            (Topology::Hexagonal, bits(&[2]), bits(&[3, 4])),
            (Topology::Hexagonal, bits(&[2, 5]), bits(&[3, 4])),
            (Topology::Hexagonal, bits(&[3]), bits(&[2, 3])),
            (Topology::TriangleEdges, bits(&[2, 3]), bits(&[2])),
            (Topology::TriangleEdges, bits(&[2]), bits(&[1])),
            (Topology::TriangleVertices, bits(&[4]), bits(&[3, 4, 5])),
            (Topology::TriangleVertices, bits(&[5]), bits(&[4, 5, 6])),
            (Topology::TriangleVertices, bits(&[4, 6]), bits(&[4, 5])),
        ];
        let mut settled = [0; 5];
        for (topology, birth, survive) in cases {
            for seed in 1..=6 {
                let mut w = World::new();
                w.set_topology(topology);
                assert!(w.set_rule(birth, survive));
                w.soup(0, 0, 32, 32, 0.3, seed * 7919);
                let status = run_until_settled(&mut w, 3000);
                if !status.settled() {
                    continue;
                }
                settled[topology as usize] += 1;
                let population = w.population;
                match status {
                    Status::Empty | Status::Still => {
                        let hash = w.field_hash();
                        for _ in 0..50 {
                            w.step();
                        }
                        assert_eq!(w.field_hash(), hash, "{topology:?} {status:?}");
                    }
                    Status::Oscillating { period } => {
                        let hash = w.field_hash();
                        for _ in 0..period * 20 {
                            w.step();
                        }
                        assert_eq!(w.field_hash(), hash, "{topology:?} {status:?}");
                    }
                    Status::Drifting { period, .. } => {
                        for _ in 0..200 {
                            for _ in 0..period {
                                w.step();
                            }
                            assert_eq!(w.population, population, "{topology:?} {status:?}");
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        // Moore is covered by the tests above; all others must have been exercised.
        assert!(settled[1..].iter().all(|&n| n > 0), "settled per grid: {settled:?}");
    }

    fn torus(width: i32, height: i32, cells: &[(i32, i32)]) -> World {
        let mut w = World::new();
        assert!(w.set_torus(width, height));
        for &(x, y) in cells {
            w.set(x, y, true);
        }
        w
    }

    #[test]
    fn glider_on_a_torus_oscillates_with_its_round_trip() {
        // A glider moves one cell per four generations along both axes, so it is
        // back after four times the least common multiple of the sides.
        for (width, height, period) in [(64, 64, 256), (64, 96, 768), (1024, 1024, 4096), (70, 66, 9240)] {
            let mut w = torus(width, height, &GLIDER);
            let status = run_until_settled(&mut w, 60_000);
            assert_eq!(status, Status::Oscillating { period }, "{width}x{height}");
            let hash = w.field_hash();
            for _ in 0..period {
                w.step();
            }
            assert_eq!(w.field_hash(), hash);
        }
    }

    #[test]
    fn empty_torus_under_birth_from_nothing_is_not_extinct() {
        let mut w = torus(64, 64, &[]);
        assert!(w.set_rule(1, 0));
        // Dark and full in turns.
        assert_eq!(run_until_settled(&mut w, 2000), Status::Oscillating { period: 2 });
    }

    /// Whatever is claimed on any torus has to stay true far into the future.
    #[test]
    fn torus_claims_hold_on_every_grid() {
        let bits = |counts: &[u32]| counts.iter().fold(0u16, |m, c| m | 1 << c);
        let cases = [
            (Topology::Moore, bits(&[3]), bits(&[2, 3])),
            (Topology::VonNeumann, bits(&[3]), bits(&[1, 2])),
            (Topology::Hexagonal, bits(&[2, 5]), bits(&[3, 4])),
            (Topology::TriangleEdges, bits(&[2]), bits(&[1])),
            (Topology::TriangleVertices, bits(&[4]), bits(&[3, 4, 5])),
        ];
        let mut settled = 0;
        for (topology, birth, survive) in cases {
            for seed in 1..=6 {
                let mut w = World::new();
                assert!(w.set_torus(48, 40));
                w.set_topology(topology);
                assert!(w.set_rule(birth, survive));
                w.soup(0, 0, 48, 40, 0.3, seed * 7919);
                let status = run_until_settled(&mut w, 6000);
                if !status.settled() {
                    continue;
                }
                settled += 1;
                assert!(!matches!(status, Status::Drifting { .. }), "{topology:?}");
                let period = match status {
                    Status::Oscillating { period } => period,
                    _ => 1,
                };
                let hash = w.field_hash();
                for _ in 0..period * 4 {
                    w.step();
                }
                assert_eq!(w.field_hash(), hash, "{topology:?} {status:?}");
            }
        }
        assert!(settled >= 10, "only {settled} runs settled");
    }

    #[test]
    fn editing_resets_the_status() {
        let mut w = world(&[(&BLOCK, 0, 0)]);
        assert_eq!(run_until_settled(&mut w, 10), Status::Still);
        w.set(50, 50, true);
        assert_eq!(w.detector.status, Status::Active);
    }

    #[test]
    fn r_pentomino_settles_with_its_known_population() {
        let mut w = world(&[(&[(1, 0), (2, 0), (0, 1), (1, 1), (1, 2)], 0, 0)]);
        let status = run_until_settled(&mut w, 4000);
        assert!(matches!(status, Status::Drifting { moving: 6, .. }), "{status:?}");
        assert_eq!(w.population, 116);
    }
}
