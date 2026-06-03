//! Domain-typed facade over the iTunes Search API.
//!
//! The browse screen needs three populated sections; the iTunes
//! Search API has no concept of "top podcasts by chart position".
//! Workaround: issue several `term=` queries with different seed
//! topics, label each with a section title, and return the lot as
//! `Vec<ChartSection>`. The UI doesn't see this — it just renders
//! sections in order.
//!
//! Section assignment is intentionally hard-coded here, not in the
//! UI or domain. Tweaking the seeds (or replacing them with a
//! genre-id rotation, a remote config, an A/B test) stays local.

use podcast_domain::{ChartSection, Podcast, SectionLayout};

use crate::itunes::{self, FetchError, SearchQuery};

/// One row's source recipe: title shown to the user, layout the
/// UI kit should use, the iTunes Search seed term, and how many
/// items to take. Kept private — the public `fetch_browse_screen`
/// returns fully-resolved `ChartSection`s.
struct SectionSeed {
    title: &'static str,
    layout: SectionLayout,
    term: &'static str,
    take: u32,
}

const SECTIONS: &[SectionSeed] = &[
    SectionSeed {
        title: "New",
        layout: SectionLayout::Featured,
        term: "interview",
        take: 5,
    },
    SectionSeed {
        title: "Top Shows",
        layout: SectionLayout::Ranked,
        term: "news",
        take: 10,
    },
    SectionSeed {
        title: "New Shows",
        layout: SectionLayout::Ranked,
        term: "comedy",
        take: 10,
    },
];

/// Fetch every browse-screen section sequentially. Sequential
/// (not parallel) on purpose — the API is fast enough that the
/// extra complexity of a worker-pool for 3 requests isn't worth
/// it, and the iTunes endpoint is rate-limited per-IP so spraying
/// requests in parallel can trigger 403s.
///
/// Called from a `run_blocking` worker; the awaiting main-thread
/// signal flips to `Ready` once every section resolved. A single
/// failed section currently aborts the whole load — empty-state
/// for partial success is a follow-up.
pub fn fetch_browse_screen() -> Result<Vec<ChartSection>, FetchError> {
    let mut sections = Vec::with_capacity(SECTIONS.len());
    for seed in SECTIONS {
        let items = itunes::search_blocking(SearchQuery {
            term: seed.term,
            limit: seed.take,
        })?;
        let items = take_unique(items, seed.take as usize);
        sections.push(ChartSection {
            title: seed.title.to_string(),
            layout: seed.layout,
            items,
        });
    }
    Ok(sections)
}

/// iTunes Search occasionally returns duplicate `collectionId`s
/// across pages — strip them so list keying stays stable. Order
/// is preserved.
fn take_unique(items: Vec<Podcast>, max: usize) -> Vec<Podcast> {
    let mut seen = std::collections::HashSet::with_capacity(items.len());
    items
        .into_iter()
        .filter(|p| seen.insert(p.id))
        .take(max)
        .collect()
}
