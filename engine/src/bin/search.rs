//! Searches the hat tiling for travelling patterns under two-state rules, on the
//! engine the page uses. See `search --help`, and "Suchmodus" in Readme Entwicklung.md.

use engine::hat_world::{CellKey, HatWorld};
use engine::search::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

/// The search allocates small blocks all the time on all cores, and the allocator of
/// musl has one lock for everybody: 16 workers cost 9 times the CPU per rule of one.
/// So each thread keeps free lists of its own, and only large blocks and fresh chunks
/// go to the system. Blocks freed on another thread simply join that thread's list.
mod pool {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::ptr::null_mut;

    const CLASSES: usize = 12;
    const SMALLEST_SHIFT: usize = 4;
    const LARGEST: usize = 1 << (SMALLEST_SHIFT + CLASSES - 1);
    const CHUNK: usize = 1 << 21;

    thread_local! {
        static FREE: [Cell<*mut u8>; CLASSES] = const { [const { Cell::new(null_mut()) }; CLASSES] };
        static BUMP: Cell<(*mut u8, usize)> = const { Cell::new((null_mut(), 0)) };
    }

    pub struct Pool;

    fn class_of(layout: &Layout) -> Option<usize> {
        if layout.size() > LARGEST || layout.align() > 16 {
            return None;
        }
        let size = layout.size().max(1 << SMALLEST_SHIFT).next_power_of_two();
        Some(size.trailing_zeros() as usize - SMALLEST_SHIFT)
    }

    unsafe impl GlobalAlloc for Pool {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let Some(class) = class_of(&layout) else { return unsafe { System.alloc(layout) } };
            let head = FREE.with(|free| free[class].get());
            if !head.is_null() {
                FREE.with(|free| free[class].set(unsafe { *(head as *mut *mut u8) }));
                return head;
            }
            let size = 1usize << (class + SMALLEST_SHIFT);
            let (mut at, mut left) = BUMP.with(Cell::get);
            if left < size {
                at = unsafe { System.alloc(Layout::from_size_align_unchecked(CHUNK, 16)) };
                if at.is_null() {
                    return at;
                }
                left = CHUNK;
            }
            BUMP.with(|bump| bump.set((unsafe { at.add(size) }, left - size)));
            at
        }

        unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
            let Some(class) = class_of(&layout) else { return unsafe { System.dealloc(block, layout) } };
            FREE.with(|free| {
                unsafe { *(block as *mut *mut u8) = free[class].get() };
                free[class].set(block);
            });
        }
    }
}

#[global_allocator]
static ALLOCATOR: pool::Pool = pool::Pool;

const HELP: &str = "\
search: patterns that travel on the hat tiling, under two-state rules

  search [options]             run the search, or continue it in the same --out
  search --estimate N ...      try N rules and project how long all of them take
  search probe                 measure the tiling: rings, balls, corridors, polyhats
  search control               fly the known glider through all tests
  search replay --rules R --neighborhood H --cells a,b,c,...   run one seed again
  search merge --out DIR A B   join batches from other machines, then finalize
  search unique --out DIR      how many forms the settled finds really have, as unique.html

What to search
  --tiling hat|spectre         spectre: same graph, so same results, other shape in the page
  --neighborhood moore|edge|r2|r3   default moore
  --rules all|B36/S234,...     or R2B5-7S3-8 for r2 and r3; default all
  --seeds polyhat:3-8,corridor,soup:3-5,mutate   default all of these
                               also late:3-5 and latesoup:3-4: seeds that may burn for long,
                               and only what leaves them must pass the strict test
  --seed N                     seed of the run; default 1
  --keep stable,chaotic        classes searched after the screening, default these two
  --screen-only                only the screening, which makes the rule catalogue

Batches and time
  --batch K/N                  only the K-th of N equal slices of the rules, K from 1
  --max-rules N                only the next N rules not done yet
  --time-limit 2h              stop taking new rules after this: 90s, 45m, 2h
  --workers N                  threads; default all cores
  --out DIR                    default results/

Finished rules are never run again in the same --out, however a run ended. Rules are
tried in a fixed shuffled order, so that every slice costs about the same.

Details
  --positions N                default 20, spread over H, T, P, F and mirrored hats
  --corridors N                default 20
  --soups-each N               per size and density; default 8
  --densities 0.3,0.5,0.7      default these
  --max-seeds-per-rule N       per family of seeds; default 10000000, which all of polyhat:3-8
                               fits into; a rule that hits this is marked truncated
  --max-generations N          default 20000
  --patience N                 generations without progress; default 300
  --pop-factor F --pop-floor N --max-pop N   limit: F times the start, at least N, at most M
                               defaults 3, 24, 60
  --mutate-top N --mutate-trials N           defaults 3, 60
  --screen-soups N             default 3
  --late-generations N --late-max-pop N      limits of late seeds; defaults 2000, 300
  --files default|all|none     all: a file for every find, not just the first of a kind
  --base-url URL               for the links in SUMMARY.md; default http://localhost:5173/
  --smoke                      small and quick: 48 rules, small seeds
";

struct Options {
    values: HashMap<String, String>,
    flags: HashSet<String>,
    free: Vec<String>,
}

const FLAGS: [&str; 3] = ["screen-only", "smoke", "help"];

fn parse_args(args: Vec<String>) -> Result<Options, String> {
    let mut o = Options { values: HashMap::new(), flags: HashSet::new(), free: Vec::new() };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        if let Some(name) = arg.strip_prefix("--") {
            if FLAGS.contains(&name) {
                o.flags.insert(name.to_string());
            } else {
                let value = it.next().ok_or(format!("--{name} needs a value"))?;
                o.values.insert(name.to_string(), value);
            }
        } else {
            o.free.push(arg);
        }
    }
    Ok(o)
}

impl Options {
    fn get(&self, name: &str, default: &str) -> String {
        self.values.get(name).cloned().unwrap_or_else(|| default.to_string())
    }
    fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        match self.values.get(name) {
            Some(text) => text.parse().map_err(|_| format!("--{name}: cannot read {text}")),
            None => Ok(default),
        }
    }
}

fn parse_duration(text: &str) -> Result<Duration, String> {
    let bad = || format!("--time-limit: cannot read {text}, try 90s, 45m or 2h");
    let split = text.len().checked_sub(1).ok_or_else(bad)?;
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().map_err(|_| bad())?;
    let seconds = match unit {
        "s" => number,
        "m" => number * 60.0,
        "h" => number * 3600.0,
        _ => return Err(bad()),
    };
    Ok(Duration::from_secs_f64(seconds))
}

fn clock(seconds: f64) -> String {
    if !seconds.is_finite() {
        return "?".into();
    }
    let s = seconds.round() as u64;
    if s >= 86400 {
        format!("{}d {:02}h", s / 86400, s % 86400 / 3600)
    } else {
        format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    }
}

fn parse_seeds(text: &str) -> Result<SeedPlan, String> {
    let mut plan = SeedPlan { polyhat: None, corridor: false, soup: None, mutate: false, late: None, late_soup: None };
    let range = |part: &str, name: &str| -> Result<(usize, usize), String> {
        let (low, high) = part.split_once('-').unwrap_or((part, part));
        let pair = (low.parse().map_err(|_| format!("{name}:{part}"))?, high.parse().map_err(|_| format!("{name}:{part}"))?);
        if pair.0 < 1 || pair.0 > pair.1 {
            return Err(format!("{name}:{part} is an empty range"));
        }
        Ok(pair)
    };
    for part in text.split(',') {
        let (name, arg) = part.split_once(':').unwrap_or((part, ""));
        match name {
            "polyhat" => plan.polyhat = Some(range(if arg.is_empty() { "3-8" } else { arg }, name)?),
            "corridor" => plan.corridor = true,
            "soup" => {
                let (low, high) = range(if arg.is_empty() { "3-5" } else { arg }, name)?;
                plan.soup = Some((low as u32, high as u32));
            }
            "mutate" | "mutation" => plan.mutate = true,
            "late" => plan.late = Some(range(if arg.is_empty() { "3-5" } else { arg }, name)?),
            "latesoup" => {
                let (low, high) = range(if arg.is_empty() { "3-4" } else { arg }, name)?;
                plan.late_soup = Some((low as u32, high as u32));
            }
            other => return Err(format!("unknown seed kind {other}")),
        }
    }
    Ok(plan)
}

fn list<T: std::str::FromStr>(text: &str, name: &str) -> Result<Vec<T>, String> {
    text.split(',').map(|v| v.parse().map_err(|_| format!("--{name}: cannot read {v}"))).collect()
}

fn class_of_name(text: &str) -> Result<RuleClass, String> {
    match text {
        "explosive" => Ok(RuleClass::Explosive),
        "dies" => Ok(RuleClass::Dies),
        "stable" => Ok(RuleClass::Stable),
        "chaotic" => Ok(RuleClass::Chaotic),
        other => Err(format!("--keep: unknown class {other}")),
    }
}

struct Setup {
    config: Config,
    rules: Vec<Rule>,
    /// What decides the results, to keep runs with other settings apart.
    fingerprint: String,
    morph: u32,
    base_url: String,
    files: String,
}

fn setup(o: &Options) -> Result<Setup, String> {
    let smoke = o.flags.contains("smoke");
    let hood_name = o.get("neighborhood", "moore");
    let hood = Hood::parse(&hood_name).ok_or(format!("unknown neighbourhood {hood_name}"))?;
    let morph = match o.get("tiling", "hat").as_str() {
        "hat" => 60,
        // Only the drawing differs: see "Spectre" in Readme Entwicklung.md.
        "spectre" => {
            eprintln!("note: this is the spectre shape on the tiling of the hat, with mirrored tiles, and not the chiral spectre tiling. The cells have the same neighbours, so this finds exactly what --tiling hat finds.");
            45
        }
        other => return Err(format!("unknown tiling {other}")),
    };
    let seeds_default = if smoke { "polyhat:3-4,corridor,soup:3-3,mutate" } else { "polyhat:3-8,corridor,soup:3-5,mutate" };
    let seeds_text = o.get("seeds", seeds_default);
    let plan = parse_seeds(&seeds_text)?;
    let master_seed: u64 = o.number("seed", 1)?;
    let rules_text = o.get("rules", "all");
    let mut rules = if rules_text == "all" {
        let mut all = Rule::all(hood);
        // The order of enumeration runs from cheap to costly. Shuffled, any slice of
        // rules costs about the same, and so progress tells how long the rest takes.
        all.sort_by_cached_key(|r| (mix_seed(&[master_seed, fnv(&r.name)]), r.name.clone()));
        all
    } else {
        rules_text.split(',').map(|name| Rule::parse(name, hood)).collect::<Result<Vec<_>, _>>()?
    };
    if smoke && rules_text == "all" {
        rules.truncate(48);
    }
    let mut world = HatWorld::new();
    let positions = select_positions(&mut world, o.number("positions", if smoke { 5 } else { 20 })?);
    let corridor_count = o.number("corridors", if smoke { 4 } else { 20 })?;
    let corridors = if plan.corridor { find_corridors(&mut world, corridor_count).0 } else { Vec::new() };
    let keep_text = o.get("keep", "stable,chaotic");
    let keep = if o.flags.contains("screen-only") { Vec::new() } else { keep_text.split(',').map(class_of_name).collect::<Result<Vec<_>, _>>()? };
    let config = Config {
        hood,
        plan,
        positions,
        corridors,
        master_seed,
        pop_factor: o.number("pop-factor", 3)?,
        pop_floor: o.number("pop-floor", 24)?,
        max_pop: o.number("max-pop", 60)?,
        patience: o.number("patience", 300)?,
        max_generations: o.number("max-generations", 20_000)?,
        screen_soups: o.number("screen-soups", 3)?,
        screen_generations: 200,
        screen_rings: 6,
        keep,
        soups_each: o.number("soups-each", if smoke { 2 } else { 8 })?,
        densities: list(&o.get("densities", "0.3,0.5,0.7"), "densities")?,
        max_seeds_per_rule: o.number("max-seeds-per-rule", if smoke { 5000 } else { 10_000_000 })?,
        mutate_top: o.number("mutate-top", 3)?,
        mutate_trials: o.number("mutate-trials", if smoke { 20 } else { 60 })?,
        late_generations: o.number("late-generations", 2000)?,
        late_max_pop: o.number("late-max-pop", 300)?,
    };
    let fingerprint = format!(
        "v3|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}|{}|{}|{}|{}|{}|{}",
        hood.name(), rules_text, smoke, seeds_text, master_seed, config.positions.len(), config.corridors.len(), config.pop_factor, config.pop_floor, config.max_pop,
        config.patience, config.max_generations, config.keep.iter().map(|k| k.name()).collect::<Vec<_>>(), config.soups_each, config.max_seeds_per_rule, config.mutate_top, config.mutate_trials, config.late_generations, config.late_max_pop,
    );
    Ok(Setup { config, rules, fingerprint, morph, base_url: o.get("base-url", "http://localhost:5173/"), files: o.get("files", "default") })
}

fn esc(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn keys_json(keys: &[CellKey]) -> String {
    let parts: Vec<String> = keys.iter().map(|k| format!("[{},{},{},{},{}]", k[0], k[1], k[2], k[3], k[4])).collect();
    format!("[{}]", parts.join(","))
}

fn opt<T: std::fmt::Display>(value: &Option<T>) -> String {
    value.as_ref().map_or("null".into(), |v| v.to_string())
}

/// The page's link: see `parseLink` in src/share/link.ts.
fn link_of(find: &Find, rule: &Rule, hood: Hood, morph: u32) -> String {
    let flat: Vec<String> = find.cells.iter().flatten().map(|v| v.to_string()).collect();
    let n = if hood == Hood::Edge { "&n=1" } else { "" };
    format!("#v=1&f=hat&m={morph}{n}&r={}&c={:.2},{:.2},14&h={}", rule.name, find.center.0, find.center.1, flat.join(","))
}

fn find_json(setup: &Setup, index: usize, seq: usize, rule: &Rule, find: &Find, file: &str) -> String {
    let c = &setup.config;
    let position = find.position.map_or("null".to_string(), |at| {
        if find.position_class == "corridor" {
            let corridor = &c.corridors[at];
            format!("{{\"class\":\"corridor\",\"index\":{at},\"axis\":{:.1},\"gap\":{:.1}}}", corridor.axis, corridor.gap)
        } else {
            let p = &c.positions[at];
            format!("{{\"class\":\"{}\",\"index\":{at},\"key\":{}}}", p.class, keys_json(&[p.key]))
        }
    });
    let patches: Vec<String> = find.patches.iter().map(|p| format!("{{\"rings\":{},\"passed\":{},\"reached\":{},\"generations\":{},\"note\":\"{}\"}}", p.rings, p.passed, p.reached, p.generations, p.note)).collect();
    format!(
        "{{\"rule_index\":{index},\"seq\":{seq},\"rule\":\"{}\",\"neighborhood\":{{\"type\":\"{}\",\"radius\":{}}},\"category\":\"{}\",\"seed_type\":\"{}\",\"start_cells\":{},\"derived_from\":{},\"derived_after_generations\":{},\"position\":{position},\"soup\":{},\"mutation_seed\":{},\"master_seed\":{},\"reach_rings\":{:.1},\"reach_units\":{:.1},\"generations\":{},\"angle\":{:.2},\"angle_mod_60\":{:.2},\"min_pop\":{},\"max_pop\":{},\"period\":{},\"occurrences\":{},\"patches\":[{}],\"file\":\"{file}\",\"link\":\"{}\"}}",
        esc(&rule.name), c.hood.name(), c.hood.radius(), find.category, find.kind, keys_json(&find.cells),
        find.derived_from.as_ref().map_or("null".to_string(), |k| keys_json(k)), find.derived_generation,
        find.soup_seed.map_or("null".to_string(), |s| format!("{{\"seed\":{s},\"rings\":{},\"density\":{}}}", find.soup_rings, find.soup_density)),
        opt(&find.mutation_seed), c.master_seed, find.reach_rings, find.reach_units, find.generations, find.angle, find.angle.rem_euclid(60.0), find.min_pop, find.max_pop, find.period, find.occurrences,
        patches.join(","), link_of(find, rule, c.hood, setup.morph),
    )
}

fn catalog_json(c: &Config, index: usize, rule: &Rule, report: &RuleReport) -> String {
    let soups: Vec<String> = report.screens.iter().map(|s| format!("{{\"class\":\"{}\",\"seed\":{},\"position\":{},\"start_pop\":{},\"peak\":{},\"generations\":{},\"period\":{}}}", s.class.name(), s.seed, s.position, s.start_pop, s.peak, s.generations, s.period)).collect();
    let st = &report.stats;
    format!(
        "{{\"rule_index\":{index},\"rule\":\"{}\",\"neighborhood\":\"{}\",\"class\":\"{}\",\"searched\":{},\"seeds\":{},\"died\":{},\"still\":{},\"oscillators\":{},\"overpopulated\":{},\"merged\":{},\"stalled\":{},\"out_of_generations\":{},\"truncated\":{},\"pieces_cut_out\":{},\"mutations_improved\":{},\"mutation_reach_rings\":{:.1},\"late_generations_longest\":{},\"best_reach_rings\":{:.1},\"finds\":{},\"soups\":[{}]}}",
        esc(&rule.name), c.hood.name(), report.class.name(), report.searched, st.seeds, st.died, st.still, st.oscillators, st.overpopulated, st.merged, st.stalled, st.out_of_generations, st.truncated, st.cuts_tried, st.mutations_improved, st.mutation_reach_units / UNITS_PER_RING_FAR, st.late_generations_longest, st.best_reach_units / UNITS_PER_RING_FAR, report.finds.len(), soups.join(","),
    )
}

/// A number from a line of our own JSON, which needs no real parser.
fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let tag = format!("\"{name}\":");
    let at = line.find(&tag)? + tag.len();
    let rest = &line[at..];
    let end = rest.find(|c| c == ',' || c == '}').unwrap_or(rest.len());
    Some(rest[..end].trim_matches('"'))
}

fn read_lines(path: &Path) -> Vec<String> {
    match File::open(path) {
        Ok(file) => BufReader::new(file).lines().map_while(Result::ok).filter(|l| l.ends_with('}')).collect(),
        Err(_) => Vec::new(),
    }
}

const CATEGORY_RANK: [&str; 4] = ["glider-candidate", "traveller", "oscillator", "still-life"];

fn rank(category: &str) -> usize {
    CATEGORY_RANK.iter().position(|c| *c == category).unwrap_or(9)
}

/// Writes the sorted result files and the summary, from all finished rules so far.
fn finalize(out: &Path, command: &str, total_rules: usize, base_url: &str) -> std::io::Result<(usize, usize)> {
    let catalog = read_lines(&out.join("catalog.partial.jsonl"));
    let done: HashSet<usize> = catalog.iter().filter_map(|l| field(l, "rule_index")?.parse().ok()).collect();
    let mut by_rule: BTreeMap<usize, String> = BTreeMap::new();
    for line in catalog {
        if let Some(index) = field(&line, "rule_index").and_then(|v| v.parse().ok()) {
            by_rule.insert(index, line);
        }
    }
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    let mut truncated = 0;
    let mut catalog_text = String::new();
    for line in by_rule.values() {
        catalog_text.push_str(line);
        catalog_text.push('\n');
        *classes.entry(field(line, "class").unwrap_or("?").to_string()).or_default() += 1;
        if field(line, "truncated") == Some("true") {
            truncated += 1;
        }
    }
    fs::write(out.join("catalog.jsonl"), catalog_text)?;

    let mut results: BTreeMap<(usize, usize), String> = BTreeMap::new();
    for line in read_lines(&out.join("results.partial.jsonl")) {
        let index = field(&line, "rule_index").and_then(|v| v.parse().ok());
        let seq = field(&line, "seq").and_then(|v| v.parse().ok());
        if let (Some(index), Some(seq)) = (index, seq) {
            if done.contains(&index) {
                results.insert((index, seq), line);
            }
        }
    }
    let mut text = String::new();
    for line in results.values() {
        text.push_str(line);
        text.push('\n');
    }
    fs::write(out.join("results.jsonl"), text)?;

    let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
    for line in results.values() {
        *counts.entry(rank(field(line, "category").unwrap_or(""))).or_default() += 1;
    }
    let reach = |l: &str| field(l, "reach_rings").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let period = |l: &str| field(l, "period").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let mut best: Vec<&String> = results.values().collect();
    // Moving things by distance, then settled ones by period, as distance means nothing there.
    best.sort_by(|a, b| {
        let (ra, rb) = (rank(field(a, "category").unwrap_or("")), rank(field(b, "category").unwrap_or("")));
        let order = if ra < 2 { reach(b).total_cmp(&reach(a)) } else { period(b).cmp(&period(a)).then(reach(b).total_cmp(&reach(a))) };
        ra.cmp(&rb).then(order)
    });
    let mut md = String::from("# Suchergebnisse\n\n");
    md.push_str(&format!("{} von {} Regeln fertig.", done.len(), total_rules));
    if truncated > 0 {
        md.push_str(&format!(" Bei {truncated} Regeln griff das Limit `--max-seeds-per-rule`, sie sind also nicht vollständig durchsucht."));
    }
    md.push_str("\n\n| Regelklasse | Anzahl |\n|---|---|\n");
    for (class, count) in &classes {
        md.push_str(&format!("| {class} | {count} |\n"));
    }
    md.push_str("\n| Kategorie | Einträge |\n|---|---|\n");
    for (i, name) in CATEGORY_RANK.iter().enumerate() {
        md.push_str(&format!("| {name} | {} |\n", counts.get(&i).copied().unwrap_or(0)));
    }
    md.push_str(&format!("\nGleichartige Funde sind je Regel zusammengefasst, `occurrences` in `results.jsonl` zählt sie.\n\n## Reproduzieren\n\n```\n{command}\n```\n\n## Beste 20\n\n"));
    if best.is_empty() {
        md.push_str("Keine Funde.\n");
    } else {
        md.push_str("| # | Kategorie | Regel | Start | Ringe | Generationen | Population | Periode | Winkel | Link |\n|---|---|---|---|---|---|---|---|---|---|\n");
        for (i, line) in best.iter().take(20).enumerate() {
            let at = line.find("\"link\":\"").map(|p| p + 8).unwrap_or(0);
            let link = line[at..].trim_end_matches("\"}");
            md.push_str(&format!(
                "| {} | {} | `{}` | {} | {} | {} | {}–{} | {} | {} | [öffnen]({base_url}{link}) |\n",
                i + 1, field(line, "category").unwrap_or(""), field(line, "rule").unwrap_or(""), field(line, "seed_type").unwrap_or(""), field(line, "reach_rings").unwrap_or(""),
                field(line, "generations").unwrap_or(""), field(line, "min_pop").unwrap_or(""), field(line, "max_pop").unwrap_or(""), field(line, "period").unwrap_or(""), field(line, "angle").unwrap_or(""),
            ));
        }
    }
    fs::write(out.join("SUMMARY.md"), md)?;
    Ok((done.len(), results.len()))
}

struct Done {
    index: usize,
    report: RuleReport,
    seconds: f64,
}

/// Runs rules on all workers. `each` gets every finished rule, in order of finishing.
fn run_workers(config: &Config, rules: &[Rule], pending: &[usize], workers: usize, deadline: Option<Instant>, mut each: impl FnMut(Done)) {
    let next = AtomicUsize::new(0);
    let lock = Mutex::new(());
    let (sender, receiver) = mpsc::channel::<Done>();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let (next, lock) = (&next, &lock);
            std::thread::Builder::new().stack_size(16 << 20).spawn_scoped(scope, move || {
                let mut searcher = Searcher::new(config, lock);
                loop {
                    if deadline.is_some_and(|d| Instant::now() >= d) {
                        return;
                    }
                    let Some(&index) = pending.get(next.fetch_add(1, Ordering::Relaxed)) else { return };
                    let started = Instant::now();
                    let report = searcher.search_rule(&rules[index]);
                    if sender.send(Done { index, report, seconds: started.elapsed().as_secs_f64() }).is_err() {
                        return;
                    }
                }
            }).expect("thread starts");
        }
        drop(sender);
        for done in receiver {
            each(done);
        }
    });
}

fn command_run(o: &Options, argv: &[String]) -> Result<(), String> {
    let setup = setup(o)?;
    let workers: usize = o.number("workers", std::thread::available_parallelism().map_or(1, |n| n.get()))?;
    if let Some(text) = o.values.get("estimate") {
        return estimate(&setup, text.parse().map_err(|_| "--estimate needs a number")?, workers);
    }
    let out = PathBuf::from(o.get("out", "results"));
    fs::create_dir_all(out.join("finds")).map_err(|e| e.to_string())?;
    check_fingerprint(&out, &setup.fingerprint)?;

    // A line in the catalogue means that this rule and all its finds are complete.
    let done: HashSet<usize> = read_lines(&out.join("catalog.partial.jsonl")).iter().filter_map(|l| field(l, "rule_index")?.parse().ok()).collect();
    let (from, to) = match o.values.get("batch") {
        Some(text) => {
            let (k, n) = text.split_once('/').ok_or("--batch needs K/N")?;
            let (k, n): (usize, usize) = (k.parse().map_err(|_| "--batch needs K/N")?, n.parse().map_err(|_| "--batch needs K/N")?);
            if k < 1 || k > n {
                return Err(format!("--batch {text}: K must be from 1 to N"));
            }
            (setup.rules.len() * (k - 1) / n, setup.rules.len() * k / n)
        }
        None => (0, setup.rules.len()),
    };
    let mut pending: Vec<usize> = (from..to).filter(|i| !done.contains(i)).collect();
    if let Some(limit) = o.values.get("max-rules") {
        pending.truncate(limit.parse().map_err(|_| "--max-rules needs a number")?);
    }
    let deadline = match o.values.get("time-limit") {
        Some(text) => Some(Instant::now() + parse_duration(text)?),
        None => None,
    };
    let command = format!("search {}", argv.iter().map(|a| if a.contains(' ') { format!("'{a}'") } else { a.clone() }).collect::<Vec<_>>().join(" "));
    write_run_file(&out, &setup, &command).map_err(|e| e.to_string())?;

    let total = setup.rules.len();
    eprintln!("{} rules in all, {} done before, {} to do in this run, {workers} workers, output in {}", total, done.len(), pending.len(), out.display());

    let open = |name: &str| OpenOptions::new().create(true).append(true).open(out.join(name)).map_err(|e| e.to_string());
    let (mut results_file, mut catalog_file) = (open("results.partial.jsonl")?, open("catalog.partial.jsonl")?);
    let started = Instant::now();
    let mut last = Instant::now();
    let tty = std::io::stderr().is_terminal();
    let interval = Duration::from_secs(if tty { 1 } else { 30 });
    let (mut finished, mut seeds, mut truncated) = (0usize, 0u64, 0usize);
    let mut classes = [0usize; 4];
    let mut categories = [0usize; 4];
    let (mut best_reach, mut busy) = (0f64, 0f64);
    let mut files_seen: HashSet<String> = HashSet::new();
    let batch_size = pending.len();
    let earlier = done.len();

    run_workers(&setup.config, &setup.rules, &pending, workers, deadline, |item| {
        let rule = &setup.rules[item.index];
        finished += 1;
        busy += item.seconds;
        seeds += item.report.stats.seeds;
        truncated += item.report.stats.truncated as usize;
        classes[match item.report.class { RuleClass::Explosive => 0, RuleClass::Dies => 1, RuleClass::Stable => 2, RuleClass::Chaotic => 3 }] += 1;
        best_reach = best_reach.max(item.report.stats.best_reach_units / UNITS_PER_RING_FAR);
        let mut lines = String::new();
        for (seq, find) in item.report.finds.iter().enumerate() {
            categories[rank(find.category).min(3)] += 1;
            let name = format!("{}-{}-{seq}.json", find.category, rule.name.replace('/', "_"));
            // Everything that moves, and the first of each kind of thing that does not.
            let kind_key = format!("{}/{}/{}", rule.name, find.category, find.period);
            let write = match setup.files.as_str() {
                "all" => true,
                "none" => false,
                _ => find.category == "glider-candidate" || find.category == "traveller" || files_seen.insert(kind_key),
            };
            let line = find_json(&setup, item.index, seq, rule, find, if write { &name } else { "" });
            if write {
                let file = format!("{{\"format\":\"game-of-emergence-find\",\"version\":1,{}\n", &line[1..]);
                if let Err(e) = fs::write(out.join("finds").join(&name), file) {
                    eprintln!("cannot write {name}: {e}");
                }
            }
            lines.push_str(&line);
            lines.push('\n');
        }
        let catalog = catalog_json(&setup.config, item.index, rule, &item.report);
        // The results first: a catalogue line vouches for them.
        if results_file.write_all(lines.as_bytes()).and_then(|_| results_file.flush()).is_err()
            || writeln!(catalog_file, "{catalog}").and_then(|_| catalog_file.flush()).is_err()
        {
            eprintln!("cannot write the results, is the disk full?");
            std::process::exit(1);
        }
        if last.elapsed() >= interval || finished == batch_size {
            last = Instant::now();
            let elapsed = started.elapsed().as_secs_f64();
            let rate = finished as f64 / elapsed.max(0.001);
            let in_all = earlier + finished;
            let line = format!(
                "{in_all}/{total} rules ({:.1}%) | this run {finished}/{batch_size} | {rate:.2} rules/s | left: this run {}, all rules {} | expl {} dies {} stable {} chaotic {} | gliders {} travellers {} osc {} still {} | seeds {} | best {best_reach:.1} rings",
                100.0 * in_all as f64 / total.max(1) as f64, clock((batch_size - finished) as f64 / rate), clock((total - in_all) as f64 / rate), classes[0], classes[1], classes[2], classes[3], categories[0], categories[1], categories[2], categories[3], seeds,
            );
            if tty { eprint!("\r\x1b[2K{line}"); } else { eprintln!("{line}"); }
        }
    });
    if tty { eprintln!(); }

    let (rules_done, finds) = finalize(&out, &command, total, &setup.base_url).map_err(|e| e.to_string())?;
    let elapsed = started.elapsed().as_secs_f64();
    let verdict = if rules_done == total { "all rules are done" } else if finished < batch_size { "stopped at the time limit" } else { "this batch is done" };
    eprintln!("{verdict}: {finished} rules in {} ({:.2} CPU seconds per rule, {truncated} truncated). Now {rules_done}/{total} rules and {finds} finds in {}.", clock(elapsed), busy / finished.max(1) as f64, out.display());
    if rules_done < total {
        eprintln!("To continue: run the same command again, or with another --batch. Finished rules are skipped.");
    }
    Ok(())
}

fn check_fingerprint(out: &Path, fingerprint: &str) -> Result<(), String> {
    let path = out.join("fingerprint.txt");
    match fs::read_to_string(&path) {
        Ok(existing) if existing.trim() != fingerprint => Err(format!("{} holds a run with other settings, which must not be mixed with this one.\n  there: {}\n  here:  {fingerprint}\nPlease use another --out.", out.display(), existing.trim())),
        Ok(_) => Ok(()),
        Err(_) => fs::write(&path, fingerprint).map_err(|e| e.to_string()),
    }
}

fn write_run_file(out: &Path, setup: &Setup, command: &str) -> std::io::Result<()> {
    let c = &setup.config;
    let positions: Vec<String> = c.positions.iter().map(|p| format!("{{\"class\":\"{}\",\"key\":{}}}", p.class, keys_json(&[p.key]))).collect();
    let corridors: Vec<String> = c.corridors.iter().map(|k| format!("{{\"axis\":{:.1},\"gap\":{:.1},\"overlap\":{:.1},\"cells\":{},\"starts\":{}}}", k.axis, k.gap, k.overlap, k.cells.len(), k.starts.len())).collect();
    let text = format!(
        "{{\"engine_version\":\"{}\",\"command\":\"{}\",\"fingerprint\":\"{}\",\"master_seed\":{},\"morph\":{},\"rules\":{},\"positions\":[{}],\"corridors\":[{}]}}\n",
        env!("CARGO_PKG_VERSION"), esc(command), esc(&setup.fingerprint), c.master_seed, setup.morph, setup.rules.len(), positions.join(","), corridors.join(",")
    );
    fs::write(out.join("run.json"), text)
}

/// Tries a sample of the shuffled rules, which stands in for all of them.
fn estimate(setup: &Setup, sample: usize, workers: usize) -> Result<(), String> {
    if sample == 0 {
        return Err("--estimate needs at least 1 rule".into());
    }
    let pending: Vec<usize> = (0..sample.min(setup.rules.len())).collect();
    eprintln!("trying {} rules of {} with {workers} workers, nothing is written ...", pending.len(), setup.rules.len());
    let started = Instant::now();
    let mut times: Vec<f64> = Vec::new();
    let mut by_class: BTreeMap<&'static str, (usize, f64, u64)> = BTreeMap::new();
    let (mut truncated, mut last) = (0usize, Instant::now());
    let total_sample = pending.len();
    run_workers(&setup.config, &setup.rules, &pending, workers, None, |item| {
        times.push(item.seconds);
        let entry = by_class.entry(item.report.class.name()).or_default();
        entry.0 += 1;
        entry.1 += item.seconds;
        entry.2 += item.report.stats.seeds;
        truncated += item.report.stats.truncated as usize;
        if last.elapsed().as_secs() >= 2 {
            last = Instant::now();
            eprintln!("  {}/{total_sample}, {}", times.len(), clock(started.elapsed().as_secs_f64()));
        }
    });
    let wall = started.elapsed().as_secs_f64();
    times.sort_by(f64::total_cmp);
    let n = times.len() as f64;
    let sum: f64 = times.iter().sum();
    let mean = sum / n;
    let sd = (times.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt();
    let margin = 1.96 * sd / n.sqrt();
    let all = setup.rules.len() as f64;
    let quant = |q: f64| times[((n - 1.0) * q) as usize];
    println!("\nsample: {} rules in {} on {workers} workers", times.len(), clock(wall));
    println!("CPU seconds per rule: mean {mean:.3} (95% interval {:.3}..{:.3}), median {:.3}, 90% {:.3}, slowest {:.3}", (mean - margin).max(0.0), mean + margin, quant(0.5), quant(0.9), quant(1.0));
    println!("\n{:<10} {:>6} {:>7} {:>14} {:>14}", "class", "rules", "share", "CPU s per rule", "seeds per rule");
    for (name, (count, seconds, seeds)) in &by_class {
        println!("{name:<10} {count:>6} {:>6.1}% {:>14.3} {:>14.0}", 100.0 * *count as f64 / n, seconds / *count as f64, *seeds as f64 / *count as f64);
    }
    if truncated > 0 {
        println!("\n{truncated} of the sample hit --max-seeds-per-rule, so raising that limit costs more than this says.");
    }
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    if let Some(line) = status.lines().find(|l| l.starts_with("VmHWM")) {
        let kilobytes: f64 = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        println!("\nmemory at its highest: {:.1} GB for {workers} workers", kilobytes / 1e6);
    }
    println!("\nall {} rules, by the mean and its interval. Assumes as many cores as workers, and cores like these:", all);
    for cores in [workers, 8, 16, 32, 64, 128] {
        println!("  {cores:>4} cores: {} (between {} and {})", clock(all * mean / cores as f64), clock(all * (mean - margin).max(0.0) / cores as f64), clock(all * (mean + margin) / cores as f64));
    }
    Ok(())
}

fn command_probe() -> Result<(), String> {
    let mut world = HatWorld::new();
    let positions = select_positions(&mut world, 20);
    println!("positions:");
    for p in &positions {
        let cell = world.cell_of(p.key).unwrap();
        let c = world.center(cell);
        println!("  {:<5} at ({:.1}, {:.1})", p.class, c.0, c.1);
    }
    let (corridors, stats) = find_corridors(&mut world, 20);
    let mut lengths: BTreeMap<usize, usize> = BTreeMap::new();
    stats.lines.iter().for_each(|&l| *lengths.entry(l).or_default() += 1);
    println!("lines of mirrored hats, by hats in them: {lengths:?}");
    let mut gaps: BTreeMap<i64, usize> = BTreeMap::new();
    stats.gaps.iter().for_each(|&g| *gaps.entry(g.round() as i64).or_default() += 1);
    println!("distance to the next parallel line that overlaps at least 20 units: {gaps:?}");
    let mut wobbles = stats.wobbles.clone();
    wobbles.sort_by(f64::total_cmp);
    if let (Some(median), Some(worst)) = (wobbles.get(wobbles.len() / 2), wobbles.last()) {
        println!("how far hats of a line stray from its middle: median {median:.2}, worst {worst:.2} units");
    }
    println!("corridors found: {}, chosen {}", stats.corridors, corridors.len());
    for c in corridors.iter().take(8) {
        println!("  axis {:.1} gap {:.1} overlap {:.1}: {} hats, {} starts", c.axis, c.gap, c.overlap, c.cells.len(), c.starts.len());
    }
    println!("connected sets holding one hat, for three hats of different kinds:");
    for p in positions.iter().take(3) {
        let root = world.cell_of(p.key).unwrap();
        let area = ball(&mut world, root, 9);
        let adjacent: HashMap<u32, Vec<u32>> = area.iter().map(|&c| (c, world.touching(c).to_vec())).collect();
        let mut counts = Vec::new();
        for size in 3..=7 {
            let started = Instant::now();
            let mut count = 0u64;
            connected_sets(&adjacent, root, size, &mut |_| {
                count += 1;
                true
            });
            counts.push(format!("{size}: {count} ({:.0} ms)", started.elapsed().as_secs_f64() * 1e3));
        }
        println!("  {}: {}", p.class, counts.join(", "));
    }
    Ok(())
}

fn command_control(o: &Options) -> Result<(), String> {
    let s = setup(o)?;
    let limits = control_limits(&s.config);
    println!("known glider A from 2 cells, limits: population {} (3 x start alone would be 6), {} generations", limits.pop_cap, limits.max_generations);
    let mut passed = 0;
    for cell in [0u32, 3, 17, 41, 77, 120, 190, 260] {
        let mut world = control_world();
        place_glider_a(&mut world, cell);
        let mut trail = HashMap::new();
        let flight = run(&mut world, &limits, None, &mut trail);
        let rings = rings_between(&mut world, flight.start_cell, flight.reach_cell).unwrap_or(0);
        print!("cell {cell:>3}: {:?} after {} generations, {:.0} units = {rings} rings, population {}-{}, angle {:.2} ({:.2} mod 60), region stable {}", flight.fate, flight.generations, flight.reach, flight.min_pop, flight.max_pop, flight.angle, flight.angle.rem_euclid(60.0), flight.region_stable());
        if flight.fate == Fate::ReachedTarget && flight.region_stable() && rings as f64 >= GLIDER_RINGS {
            let patches = verify_in_patches(Hood::Moore, |w| place_glider_a(w, cell), &limits);
            let text: Vec<String> = patches.iter().map(|p| format!("{}: {} ({} rings, {} gen{})", p.rings, if p.passed { "ok" } else { "FAIL" }, p.reached, p.generations, if p.note.is_empty() { String::new() } else { format!(", {}", p.note) })).collect();
            print!(" | patches {}", text.join(", "));
            if patches.len() == PATCH_RINGS.len() && patches.iter().all(|p| p.passed) {
                passed += 1;
                println!(" => GLIDER CANDIDATE");
                continue;
            }
        }
        println!();
    }
    if passed == 0 {
        return Err("no flight of the known glider passed: the measurement is broken".into());
    }
    println!("{passed} flights passed all tests");
    Ok(())
}

fn command_replay(o: &Options) -> Result<(), String> {
    let s = setup(o)?;
    let rule = s.rules.first().ok_or("--rules is needed")?;
    if s.rules.len() != 1 {
        return Err("replay takes exactly one rule".into());
    }
    let numbers: Vec<i32> = list(&o.get("cells", ""), "cells")?;
    if numbers.is_empty() || numbers.len() % 5 != 0 {
        return Err("--cells needs five numbers per cell".into());
    }
    let mut world = control_world();
    if s.config.hood == Hood::Edge {
        world.set_edges_only(true);
    }
    world.set_masks_when_empty(rule.birth, rule.survive, s.config.hood.radius());
    let cells: Vec<u32> = numbers.chunks_exact(5).map(|k| world.cell_of([k[0], k[1], k[2], k[3], k[4]]).ok_or("no such hat")).collect::<Result<_, _>>()?;
    for &cell in &cells {
        world.set_alive(cell);
    }
    let limits = Limits {
        pop_cap: s.config.pop_cap(cells.len() as u32),
        transient: o.number("transient", 16)?,
        transient_cap: (cells.len() as u32 * 4).max(100),
        patience: s.config.patience,
        max_generations: s.config.max_generations,
        stop_reach: GLIDER_RINGS * UNITS_PER_RING_MAX,
        stop_spread: f64::INFINITY,
    };
    let mut trail = HashMap::new();
    let result = run(&mut world, &limits, None, &mut trail);
    let rings = rings_between(&mut world, result.start_cell, result.reach_cell).unwrap_or(0);
    println!("{:?} after {} generations, {:.1} units = {rings} rings, population {}-{}, angle {:.2}, region stable {}", result.fate, result.generations, result.reach, result.min_pop, result.max_pop, result.angle, result.region_stable());
    Ok(())
}

fn command_merge(o: &Options) -> Result<(), String> {
    let out = PathBuf::from(o.get("out", "results"));
    let sources = &o.free[1..];
    if sources.is_empty() {
        return Err("merge needs folders to join".into());
    }
    fs::create_dir_all(out.join("finds")).map_err(|e| e.to_string())?;
    let mut fingerprint: Option<String> = None;
    let mut seen: HashSet<usize> = read_lines(&out.join("catalog.partial.jsonl")).iter().filter_map(|l| field(l, "rule_index")?.parse().ok()).collect();
    let mut run_text = fs::read_to_string(out.join("run.json")).ok();
    let open = |name: &str| OpenOptions::new().create(true).append(true).open(out.join(name)).map_err(|e| e.to_string());
    let (mut results, mut catalog) = (open("results.partial.jsonl")?, open("catalog.partial.jsonl")?);
    for source in sources {
        let dir = Path::new(source);
        let print = fs::read_to_string(dir.join("fingerprint.txt")).map_err(|e| format!("{source}: {e}"))?;
        match &fingerprint {
            Some(first) if first != &print => return Err(format!("{source} has other settings than the first folder")),
            _ => fingerprint = Some(print.clone()),
        }
        check_fingerprint(&out, print.trim())?;
        if run_text.is_none() {
            let text = fs::read_to_string(dir.join("run.json")).map_err(|e| e.to_string())?;
            fs::write(out.join("run.json"), &text).map_err(|e| e.to_string())?;
            run_text = Some(text);
        }
        let new: HashSet<usize> = read_lines(&dir.join("catalog.partial.jsonl")).iter().filter_map(|l| field(l, "rule_index")?.parse().ok()).filter(|i| !seen.contains(i)).collect();
        for line in read_lines(&dir.join("results.partial.jsonl")) {
            if field(&line, "rule_index").and_then(|v| v.parse().ok()).is_some_and(|i: usize| new.contains(&i)) {
                writeln!(results, "{line}").map_err(|e| e.to_string())?;
                if let Some(name) = field(&line, "file").filter(|n| !n.is_empty()) {
                    let _ = fs::copy(dir.join("finds").join(name), out.join("finds").join(name));
                }
            }
        }
        for line in read_lines(&dir.join("catalog.partial.jsonl")) {
            if field(&line, "rule_index").and_then(|v| v.parse().ok()).is_some_and(|i: usize| new.contains(&i)) {
                writeln!(catalog, "{line}").map_err(|e| e.to_string())?;
            }
        }
        seen.extend(new);
    }
    let text = run_text.unwrap_or_default();
    let total = field(&text, "rules").and_then(|v| v.parse().ok()).unwrap_or(seen.len());
    let command = text.split("\"command\":\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("").to_string();
    let (rules_done, finds) = finalize(&out, &command, total, &o.get("base-url", "http://localhost:5173/")).map_err(|e| e.to_string())?;
    eprintln!("{rules_done}/{total} rules and {finds} finds in {}", out.display());
    Ok(())
}

/// What one line of `results.jsonl` settled into.
struct Settled {
    line: usize,
    form: Vec<[i32; 3]>,
    period: u32,
    min_pop: usize,
    max_pop: usize,
    cells: usize,
}

/// A list of numbers from a line of our own JSON.
fn start_cells(line: &str) -> Option<Vec<CellKey>> {
    let from = line.find("\"start_cells\":[")? + 15;
    let rest = &line[from..];
    let end = rest.find("]]")? + 1;
    let numbers: Vec<i32> = rest[..end].split(|c: char| !(c.is_ascii_digit() || c == '-')).filter(|t| !t.is_empty()).map(|t| t.parse().ok()).collect::<Option<_>>()?;
    Some(numbers.chunks_exact(5).map(|k| [k[0], k[1], k[2], k[3], k[4]]).collect())
}

/// Runs a find again up to its cycle, and describes the cycle by its emptiest moments.
fn settle(world: &mut HatWorld, line: usize, text: &str, hood: Hood) -> Option<Settled> {
    let rule = Rule::parse(field(text, "rule")?, hood).ok()?;
    let period: u32 = field(text, "period")?.parse().ok()?;
    let generations: u32 = field(text, "generations")?.parse().ok()?;
    world.reset_when_ruled();
    world.set_masks_when_empty(rule.birth, rule.survive, hood.radius());
    for key in start_cells(text)? {
        let cell = world.cell_of(key)?;
        world.set_alive(cell);
    }
    for _ in 0..generations {
        world.step();
    }
    let mut phases: Vec<Vec<u32>> = Vec::new();
    let mut visited: HashSet<u32> = HashSet::new();
    let (mut min_pop, mut max_pop) = (usize::MAX, 0);
    for _ in 0..period.max(1) {
        let live = world.live();
        visited.extend(live.iter().copied());
        if live.len() < min_pop {
            min_pop = live.len();
            phases.clear();
        }
        max_pop = max_pop.max(live.len());
        if live.len() == min_pop {
            phases.push(live.to_vec());
        }
        world.step();
    }
    let form = phases.iter().map(|phase| canonical_form(&phase.iter().map(|&c| world.key_of(c)).collect::<Vec<_>>())).min()?;
    Some(Settled { line, form, period, min_pop, max_pop, cells: visited.len() })
}

#[derive(Default)]
struct FormEntry {
    period: u32,
    min_pop: usize,
    max_pop: usize,
    cells: usize,
    entries: usize,
    occurrences: u64,
    rules: HashSet<String>,
    example: usize,
}

/// How many different forms lie behind the entries of a finished run. The search only
/// keeps one entry per rule, period and population, so this is a lower bound.
fn command_unique(o: &Options) -> Result<(), String> {
    let out = PathBuf::from(o.get("out", "results"));
    let lines = read_lines(&out.join("results.jsonl"));
    if lines.is_empty() {
        return Err(format!("no results.jsonl with finds in {}", out.display()));
    }
    let hood = field(&lines[0], "type").and_then(Hood::parse).ok_or("cannot read the neighbourhood")?;
    let settled_lines: Vec<usize> = (0..lines.len()).filter(|&i| matches!(field(&lines[i], "category"), Some("oscillator" | "still-life"))).collect();
    let workers: usize = o.number("workers", std::thread::available_parallelism().map_or(1, |n| n.get()))?;
    eprintln!("running {} finds again on {workers} workers ...", settled_lines.len());
    let (next, done) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let started = Instant::now();
    let parts: Vec<Vec<Settled>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let (next, done, lines, settled_lines) = (&next, &done, &lines, &settled_lines);
                scope.spawn(move || {
                    let fresh = || {
                        let mut world = control_world();
                        world.observing = false;
                        if hood == Hood::Edge {
                            world.set_edges_only(true);
                        }
                        world
                    };
                    let mut world = fresh();
                    let mut found = Vec::new();
                    while let Some(&line) = settled_lines.get(next.fetch_add(1, Ordering::Relaxed)) {
                        if world.cell_count() > 400_000 {
                            world = fresh();
                        }
                        found.extend(settle(&mut world, line, &lines[line], hood));
                        let count = done.fetch_add(1, Ordering::Relaxed) + 1;
                        if count % 20_000 == 0 {
                            eprintln!("  {count}/{} in {}", settled_lines.len(), clock(started.elapsed().as_secs_f64()));
                        }
                    }
                    found
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("worker finishes")).collect()
    });
    let mut settled: Vec<Settled> = parts.into_iter().flatten().collect();
    settled.sort_by_key(|s| s.line);
    let mut forms: HashMap<(u32, Vec<[i32; 3]>), FormEntry> = HashMap::new();
    for s in settled {
        let line = &lines[s.line];
        let entry = forms.entry((s.period, s.form)).or_default();
        if entry.entries == 0 {
            (entry.period, entry.min_pop, entry.max_pop, entry.cells, entry.example) = (s.period, s.min_pop, s.max_pop, s.cells, s.line);
        }
        entry.entries += 1;
        entry.occurrences += field(line, "occurrences").and_then(|v| v.parse().ok()).unwrap_or(1);
        entry.rules.insert(field(line, "rule").unwrap_or("").to_string());
    }
    let mut list: Vec<FormEntry> = forms.into_values().collect();
    list.sort_by(|a, b| b.period.cmp(&a.period).then(b.rules.len().cmp(&a.rules.len())).then(a.example.cmp(&b.example)));

    let oscillators = list.iter().filter(|f| f.period > 1).count();
    let still = list.len() - oscillators;
    println!("{} entries: {} forms of oscillators and {} of still lifes", settled_lines.len(), oscillators, still);
    let mut by_period: BTreeMap<u32, usize> = BTreeMap::new();
    list.iter().filter(|f| f.period > 1).for_each(|f| *by_period.entry(f.period).or_default() += 1);
    let common: Vec<String> = by_period.iter().take(12).map(|(p, n)| format!("{p}: {n}")).collect();
    println!("forms by period: {} ...", common.join(", "));
    println!("different periods: {}, longest {}", by_period.len(), by_period.keys().last().copied().unwrap_or(0));
    let shared = list.iter().filter(|f| f.rules.len() > 1).count();
    println!("forms found under more than one rule: {shared}, under most rules: {}", list.iter().map(|f| f.rules.len()).max().unwrap_or(0));

    const TABLE_LIMIT: usize = 5000;
    let base_url = o.get("base-url", "http://localhost:5173/");
    let mut rows = String::new();
    let mut jsonl = String::new();
    for (i, f) in list.iter().enumerate() {
        let line = &lines[f.example];
        let link = line.find("\"link\":\"").map_or("", |p| line[p + 8..].trim_end_matches("\"}"));
        let rule = field(line, "rule").unwrap_or("");
        jsonl.push_str(&format!(
            "{{\"form\":{i},\"period\":{},\"min_pop\":{},\"max_pop\":{},\"cells_visited\":{},\"entries\":{},\"occurrences\":{},\"rules\":{},\"example_rule\":\"{rule}\",\"settled_after\":{},\"link\":\"{link}\"}}\n",
            f.period, f.min_pop, f.max_pop, f.cells, f.entries, f.occurrences, f.rules.len(), field(line, "generations").unwrap_or("0")
        ));
        if i < TABLE_LIMIT {
            rows.push_str(&format!(
                "<tr><td>{}</td><td>{}–{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><code>{rule}</code></td><td><a href=\"{base_url}{link}\">open</a></td></tr>\n",
                f.period, f.min_pop, f.max_pop, f.cells, f.rules.len(), f.occurrences, field(line, "generations").unwrap_or("0")
            ));
        }
    }
    fs::write(out.join("unique.jsonl"), jsonl).map_err(|e| e.to_string())?;
    let note = if list.len() > TABLE_LIMIT { format!("Showing the {TABLE_LIMIT} with the longest periods of {} forms, all of them are in unique.jsonl.", list.len()) } else { format!("{} forms.", list.len()) };
    let page = format!(
        "<!doctype html><meta charset=utf-8><title>Forms in {}</title><style>body{{font:14px system-ui;margin:24px;color:#222;background:#fff}}table{{border-collapse:collapse}}th,td{{padding:3px 12px;text-align:right;border-bottom:1px solid #eee}}th{{cursor:pointer;position:sticky;top:0;background:#fff}}code{{font-size:13px}}input{{padding:6px;margin:8px 0;width:260px}}</style><h1>{oscillators} oscillators, {still} still lifes</h1><p>{note} Run <code>npm run dev</code> for the links, press Space on the page to start. A click on a heading sorts.</p><input id=f placeholder=\"Filter, e.g. B35/S\"><table id=t><thead><tr><th>Period<th>Population<th>Cells used<th>Rules<th>Seen<th>Settles after<th>First rule<th></thead><tbody>\n{rows}</tbody></table><script>const b=document.querySelector('#t tbody');document.querySelector('#f').oninput=e=>{{for(const r of b.rows)r.hidden=!r.textContent.includes(e.target.value)}};document.querySelectorAll('th').forEach((h,c)=>h.onclick=()=>{{const n=r=>parseFloat(r.cells[c].textContent)||0;[...b.rows].sort((x,y)=>n(y)-n(x)).forEach(r=>b.append(r))}})</script>",
        out.display()
    );
    fs::write(out.join("unique.html"), page).map_err(|e| e.to_string())?;
    println!("wrote unique.html and unique.jsonl in {}", out.display());
    Ok(())
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let result = parse_args(argv.clone()).and_then(|o| {
        if o.flags.contains("help") {
            print!("{HELP}");
            return Ok(());
        }
        match o.free.first().map(String::as_str) {
            None => command_run(&o, &argv),
            Some("probe") => command_probe(),
            Some("control") => command_control(&o),
            Some("replay") => command_replay(&o),
            Some("merge") => command_merge(&o),
            Some("unique") => command_unique(&o),
            Some(other) => Err(format!("unknown command {other}, see --help")),
        }
    });
    if let Err(message) = result {
        eprintln!("search: {message}");
        std::process::exit(1);
    }
}

