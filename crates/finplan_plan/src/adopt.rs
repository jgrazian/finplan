//! Where an imported plan's assumptions land in the library it joins.
//!
//! An archive carries copies of the library rows its plan uses: return
//! profiles with their distributions, the inflation profile, the tax config.
//! Importing one, or moving a plan between the cloud and this device (an
//! export and an import), must not grow the library by a copy of each every
//! time. So an imported row whose name and contents are already in the
//! library is pointed at that row, and only a row that is not there is added.
//!
//! "Contents" are everything a run reads. A same-named row with different
//! numbers is never substituted, since that would quietly change the plan:
//! it is added beside the existing one under a tagged name (`"<name> [<tag>]"`).
//! Matching compares names with those tags stripped, so a plan that has made
//! the trip before still finds the original row.
//!
//! Both homes use this: the server's import route and the browser engine's
//! `restore_plan`.

use std::collections::{HashMap, HashSet};

use crate::graph::{DistributionRow, ScenarioGraph};
use crate::library::Library;

/// The library rows an imported plan can use as they are. Keys are the
/// archive's ids; values are the library's.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Adoption {
    pub return_profiles: HashMap<i64, i64>,
    pub inflation_profile: Option<i64>,
    pub tax_config: Option<i64>,
}

impl Adoption {
    /// The graph's distributions that still have to be added: those of the
    /// return profiles and the inflation profile not matched, with the bull
    /// and bear children of a regime-switching one.
    pub fn new_distributions(&self, graph: &ScenarioGraph) -> HashSet<i64> {
        let mut roots: Vec<i64> = graph
            .return_profiles
            .values()
            .filter(|p| !self.return_profiles.contains_key(&p.id))
            .map(|p| p.distribution_id)
            .collect();
        if self.inflation_profile.is_none() {
            roots.extend(graph.inflation_distribution_id);
        }
        let mut needed = HashSet::new();
        while let Some(id) = roots.pop() {
            if needed.insert(id)
                && let Some(row) = graph.distributions.get(&id)
            {
                roots.extend(row.bull_id);
                roots.extend(row.bear_id);
            }
        }
        needed
    }
}

/// Which of `graph`'s assumptions are already in `library`, by name (tags
/// stripped) and by every value a run reads. Where several rows qualify, the
/// one named exactly the untagged name wins, then the oldest.
pub fn adopt(graph: &ScenarioGraph, library: &Library) -> Adoption {
    let ours: HashMap<i64, &DistributionRow> =
        library.distributions.iter().map(|d| (d.id, d)).collect();
    let same = |theirs: i64, mine: i64| same_distribution(graph, theirs, &ours, mine, 0);

    let mut adoption = Adoption::default();
    for profile in graph.return_profiles.values() {
        let found = best(
            &profile.name,
            library
                .return_profiles
                .iter()
                .filter(|p| {
                    p.asset_class == profile.asset_class
                        && same(profile.distribution_id, p.distribution_id)
                })
                .map(|p| (p.id, p.name.as_str())),
        );
        if let Some(id) = found {
            adoption.return_profiles.insert(profile.id, id);
        }
    }

    if let Some(distribution) = graph.inflation_distribution_id {
        let name = graph
            .inflation_profile_name
            .as_deref()
            .unwrap_or(IMPORTED_INFLATION);
        adoption.inflation_profile = best(
            name,
            library
                .inflation_profiles
                .iter()
                .filter(|p| same(distribution, p.distribution_id))
                .map(|p| (p.id, p.name.as_str())),
        );
    }

    if let Some(tax) = &graph.tax_config {
        adoption.tax_config = best(
            &tax.name,
            library
                .tax_configs
                .iter()
                .filter(|t| {
                    t.state_rate == tax.state_rate
                        && t.capital_gains_rate == tax.capital_gains_rate
                        && t.early_withdrawal_penalty_rate == tax.early_withdrawal_penalty_rate
                        && t.standard_deduction == tax.standard_deduction
                        && t.age_65_extra_deduction == tax.age_65_extra_deduction
                        && t.federal_brackets.len() == graph.tax_brackets.len()
                        && t.federal_brackets
                            .iter()
                            .zip(&graph.tax_brackets)
                            .all(|(a, b)| a.threshold == b.threshold && a.rate == b.rate)
                })
                .map(|t| (t.id, t.name.as_str())),
        );
    }
    adoption
}

/// The name an archived inflation profile goes by when the archive has none.
pub const IMPORTED_INFLATION: &str = "Imported inflation";

/// The candidate whose untagged name is `name`'s: an exact match first, then
/// the lowest id.
fn best<'a>(name: &str, candidates: impl Iterator<Item = (i64, &'a str)>) -> Option<i64> {
    let base = untagged(name);
    candidates
        .filter(|(_, n)| untagged(n) == base)
        .min_by_key(|(id, n)| (*n != base, *id))
        .map(|(id, _)| id)
}

/// `name` without the `" [<tag>]"` endings earlier imports gave it, where a
/// tag is eight hex digits, optionally followed by `:<digits>`.
pub fn untagged(name: &str) -> &str {
    let mut rest = name.trim_end();
    while let Some(head) = rest.strip_suffix(']')
        && let Some((head, tag)) = head.rsplit_once(" [")
        && is_tag(tag)
    {
        rest = head.trim_end();
    }
    rest
}

fn is_tag(tag: &str) -> bool {
    let (hex, source) = match tag.split_once(':') {
        Some((hex, source)) => (hex, Some(source)),
        None => (tag, None),
    };
    hex.len() == 8
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
        && source.is_none_or(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

/// The name a newly added row is given: its untagged name when no row of
/// that kind has it, otherwise that name with `tag` appended.
pub fn import_name<'a>(name: &str, tag: &str, mut taken: impl Iterator<Item = &'a str>) -> String {
    let base = untagged(name);
    if taken.any(|n| n == base) {
        format!("{base} [{tag}]")
    } else {
        base.to_string()
    }
}

/// Regime trees are shallow; this bounds a malformed cycle.
const MAX_DEPTH: usize = 8;

fn same_distribution(
    graph: &ScenarioGraph,
    theirs: i64,
    ours: &HashMap<i64, &DistributionRow>,
    mine: i64,
    depth: usize,
) -> bool {
    let (Some(a), Some(b)) = (graph.distributions.get(&theirs), ours.get(&mine)) else {
        return false;
    };
    let child = |x: Option<i64>, y: Option<i64>| match (x, y) {
        (None, None) => true,
        (Some(x), Some(y)) => depth < MAX_DEPTH && same_distribution(graph, x, ours, y, depth + 1),
        _ => false,
    };
    a.kind == b.kind
        && a.rate == b.rate
        && a.mean == b.mean
        && a.std_dev == b.std_dev
        && a.scale == b.scale
        && a.df == b.df
        && a.bull_to_bear_prob == b.bull_to_bear_prob
        && a.bear_to_bull_prob == b.bear_to_bull_prob
        && a.history_preset == b.history_preset
        && a.block_size == b.block_size
        && child(a.bull_id, b.bull_id)
        && child(a.bear_id, b.bear_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::new_plan;
    use crate::library::{LibraryReturnProfile, seed};
    use crate::specs::scenarios::CreateScenario;

    #[test]
    fn untagged_strips_every_import_tag() {
        assert_eq!(
            untagged("Cash / T-Bills [388ec8d3:1] [54249055:9] [4df6fb52:99]"),
            "Cash / T-Bills"
        );
        assert_eq!(untagged("Federal 2026 [deadbeef]"), "Federal 2026");
        // Not import tags: kept.
        assert_eq!(untagged("Bonds [EU]"), "Bonds [EU]");
        assert_eq!(untagged("Mix [12345678:]"), "Mix [12345678:]");
    }

    #[test]
    fn import_name_tags_only_on_a_clash() {
        let names = ["S&P 500", "Bonds"];
        assert_eq!(
            import_name("REITs [abcdef01:3]", "0badf00d", names.into_iter()),
            "REITs"
        );
        assert_eq!(
            import_name("Bonds [abcdef01:3]", "0badf00d", names.into_iter()),
            "Bonds [0badf00d]"
        );
    }

    /// A plan on the starter library, with an inflation profile and a tax config.
    fn plan_and_library() -> (ScenarioGraph, Library) {
        let library = seed();
        let body: CreateScenario = serde_json::from_value(serde_json::json!({
            "name": "Trip", "start_date": "2026-01-01",
            "inflation_profile_id": 2, "tax_config_id": 1,
        }))
        .unwrap();
        let graph = new_plan(&body, &library, 1, "2026-10-03 00:00:00").unwrap();
        (graph, library)
    }

    #[test]
    fn a_plan_that_made_the_trip_finds_the_originals() {
        let (mut graph, mut library) = plan_and_library();
        // The plan's copies carry the tags of two earlier trips.
        for p in graph.return_profiles.values_mut() {
            p.name = format!("{} [388ec8d3:{}] [54249055:9]", p.name, p.id);
        }
        if let Some(tax) = &mut graph.tax_config {
            tax.name = format!("{} [4df6fb52]", tax.name);
        }
        // A tagged duplicate already sits in the library; the original wins.
        let first = library.return_profiles[0].clone();
        library.return_profiles.push(LibraryReturnProfile {
            id: 9_999,
            name: format!("{} [388ec8d3:1]", first.name),
            ..first.clone()
        });

        let adoption = adopt(&graph, &library);
        assert_eq!(adoption.return_profiles.len(), graph.return_profiles.len());
        for (theirs, mine) in &adoption.return_profiles {
            assert_eq!(theirs, mine, "each profile maps onto itself");
        }
        assert_eq!(adoption.inflation_profile, Some(2));
        assert_eq!(adoption.tax_config, Some(1));
        assert!(adoption.new_distributions(&graph).is_empty());
    }

    #[test]
    fn different_numbers_under_the_same_name_are_added() {
        let (mut graph, library) = plan_and_library();
        let profile = graph.return_profiles[&1].clone();
        let d = graph
            .distributions
            .get_mut(&profile.distribution_id)
            .unwrap();
        d.mean = d.mean.map(|m| m + 0.01);
        d.rate = d.rate.map(|r| r + 0.01);
        if let Some(tax) = &mut graph.tax_config {
            tax.state_rate += 0.01;
        }
        let adoption = adopt(&graph, &library);
        assert!(!adoption.return_profiles.contains_key(&profile.id));
        assert_eq!(
            adoption.return_profiles.len(),
            graph.return_profiles.len() - 1
        );
        assert_eq!(adoption.tax_config, None);
        assert!(
            adoption
                .new_distributions(&graph)
                .contains(&profile.distribution_id)
        );
    }
}
