//! Settings search: every page lists its settings as [`SearchEntry`]s and the
//! page shell filters the combined list.

use super::Section;

/// One searchable setting. Each page exposes `SEARCH: &[SearchEntry]`.
pub struct SearchEntry {
    pub title: &'static str,
    pub description: &'static str,
    /// Aliases and option names people may remember instead of the title.
    pub keywords: &'static [&'static str],
    /// The page that shows this setting.
    pub section: Section,
}

impl SearchEntry {
    fn haystack(&self) -> String {
        format!("{} {} {}", self.title, self.description, self.keywords.join(" ")).to_lowercase()
    }
}

/// Entries matching every whitespace-separated word of `query`, title matches
/// first, otherwise in page order. An empty query matches nothing.
pub fn search(query: &str) -> Vec<&'static SearchEntry> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut matches: Vec<(bool, &'static SearchEntry)> = Section::ALL
        .into_iter()
        .flat_map(|section| section.search_entries())
        .filter_map(|entry| {
            let haystack = entry.haystack();
            words.iter().all(|word| haystack.contains(word.as_str())).then(|| {
                let title = entry.title.to_lowercase();
                (words.iter().all(|word| title.contains(word.as_str())), entry)
            })
        })
        .collect();
    // Stable, so page order is kept within each group.
    matches.sort_by_key(|(in_title, _)| !*in_title);
    matches.into_iter().map(|(_, entry)| entry).collect()
}
