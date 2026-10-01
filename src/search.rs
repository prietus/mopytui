//! Search results model: hits grouped by source (Tidal / Local) and, inside
//! each source, by kind (albums / artists / tracks), plus the row list the
//! results panel renders (section headers + hits) and header-skipping
//! navigation over it.

use crate::app::SearchHit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearchSource {
    Tidal,
    Local,
    Other,
}

impl SearchSource {
    pub fn from_uri(uri: &str) -> Self {
        if uri.starts_with("tidal:") {
            Self::Tidal
        } else if uri.starts_with("local:") || uri.starts_with("file:") || uri.starts_with("m3u:") {
            Self::Local
        } else {
            Self::Other
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Tidal => "TIDAL",
            Self::Local => "LOCAL",
            Self::Other => "OTHER",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearchKind {
    Album,
    Artist,
    Track,
}

impl SearchKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Album => "ÁLBUMES",
            Self::Artist => "ARTISTAS",
            Self::Track => "PISTAS",
        }
    }
}

impl SearchHit {
    pub fn uri(&self) -> &str {
        match self {
            SearchHit::Track(t) => &t.uri,
            SearchHit::Album(a) => a.uri.as_deref().unwrap_or(""),
            SearchHit::Artist(a) => a.uri.as_deref().unwrap_or(""),
        }
    }

    pub fn kind(&self) -> SearchKind {
        match self {
            SearchHit::Album(_) => SearchKind::Album,
            SearchHit::Artist(_) => SearchKind::Artist,
            SearchHit::Track(_) => SearchKind::Track,
        }
    }

    pub fn source(&self) -> SearchSource {
        SearchSource::from_uri(self.uri())
    }
}

impl SearchHit {
    /// URI whose track list the detail panel shows: the album itself, or the
    /// album of a track. Artists have none.
    pub fn detail_key(&self) -> Option<String> {
        let uri = match self {
            SearchHit::Album(a) => a.uri.clone(),
            SearchHit::Track(t) => t.album.as_ref().and_then(|a| a.uri.clone()),
            SearchHit::Artist(_) => None,
        };
        uri.filter(|u| !u.is_empty())
    }

    /// URI to ask `library.get_images` for the cover shown in the detail
    /// panel: the album's, falling back to the item's own.
    pub fn cover_key(&self) -> Option<String> {
        self.detail_key()
            .or_else(|| Some(self.uri().to_string()).filter(|u| !u.is_empty()))
    }
}

/// One line of the results panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchRow {
    /// `── TIDAL · 17 ──`
    Source { label: &'static str, count: usize },
    /// `ÁLBUMES (3)`
    Kind { label: &'static str, count: usize },
    /// Index into the grouped hit list.
    Hit(usize),
}

/// Sort `hits` by (source, kind) — keeping the backend's order inside each
/// group — and build the rows to render them with section headers.
pub fn group_hits(mut hits: Vec<SearchHit>) -> (Vec<SearchHit>, Vec<SearchRow>) {
    hits.sort_by_key(|h| (h.source(), h.kind()));
    let mut rows = Vec::new();
    let mut i = 0;
    while i < hits.len() {
        let source = hits[i].source();
        let source_end = hits[i..].iter().position(|h| h.source() != source).map_or(hits.len(), |n| i + n);
        rows.push(SearchRow::Source { label: source.label(), count: source_end - i });
        while i < source_end {
            let kind = hits[i].kind();
            let kind_end = hits[i..source_end].iter().position(|h| h.kind() != kind).map_or(source_end, |n| i + n);
            rows.push(SearchRow::Kind { label: kind.label(), count: kind_end - i });
            rows.extend((i..kind_end).map(SearchRow::Hit));
            i = kind_end;
        }
    }
    (hits, rows)
}

/// Row index of the first hit, if any.
pub fn first_hit_row(rows: &[SearchRow]) -> Option<usize> {
    rows.iter().position(|r| matches!(r, SearchRow::Hit(_)))
}

/// Move from row `cur` to the nearest hit row in the direction of `delta`
/// (`> 0` down, `< 0` up), skipping headers. `None` when there is none.
pub fn next_hit_row(rows: &[SearchRow], cur: Option<usize>, delta: i32) -> Option<usize> {
    let step = |i: usize| -> Option<usize> {
        if delta > 0 { (i + 1 < rows.len()).then_some(i + 1) } else { i.checked_sub(1) }
    };
    let mut i = match cur {
        Some(c) => step(c)?,
        None => return first_hit_row(rows),
    };
    loop {
        if matches!(rows[i], SearchRow::Hit(_)) {
            return Some(i);
        }
        i = step(i)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mopidy::models::{Album, Artist, Track};

    fn track(uri: &str) -> SearchHit {
        SearchHit::Track(Track { uri: uri.into(), ..Default::default() })
    }
    fn album(uri: &str) -> SearchHit {
        SearchHit::Album(Album { uri: Some(uri.into()), ..Default::default() })
    }
    fn artist(uri: &str) -> SearchHit {
        SearchHit::Artist(Artist { uri: Some(uri.into()), ..Default::default() })
    }

    fn sample() -> Vec<SearchHit> {
        vec![
            track("local:track:a"),
            album("tidal:album:1"),
            track("tidal:track:1:2:3"),
            artist("tidal:artist:9"),
            album("local:album:md5:x"),
            album("tidal:album:2"),
        ]
    }

    #[test]
    fn groups_by_source_then_kind_with_counts() {
        let (hits, rows) = group_hits(sample());
        assert_eq!(hits.len(), 6);
        assert_eq!(
            rows,
            vec![
                SearchRow::Source { label: "TIDAL", count: 4 },
                SearchRow::Kind { label: "ÁLBUMES", count: 2 },
                SearchRow::Hit(0),
                SearchRow::Hit(1),
                SearchRow::Kind { label: "ARTISTAS", count: 1 },
                SearchRow::Hit(2),
                SearchRow::Kind { label: "PISTAS", count: 1 },
                SearchRow::Hit(3),
                SearchRow::Source { label: "LOCAL", count: 2 },
                SearchRow::Kind { label: "ÁLBUMES", count: 1 },
                SearchRow::Hit(4),
                SearchRow::Kind { label: "PISTAS", count: 1 },
                SearchRow::Hit(5),
            ]
        );
        // Backend order is kept inside a group.
        assert_eq!(hits[0].uri(), "tidal:album:1");
        assert_eq!(hits[1].uri(), "tidal:album:2");
    }

    #[test]
    fn empty_input_has_no_rows() {
        let (hits, rows) = group_hits(Vec::new());
        assert!(hits.is_empty() && rows.is_empty());
        assert_eq!(first_hit_row(&rows), None);
        assert_eq!(next_hit_row(&rows, None, 1), None);
    }

    #[test]
    fn navigation_skips_headers_and_stops_at_the_ends() {
        let (_, rows) = group_hits(sample());
        let first = first_hit_row(&rows).unwrap();
        assert_eq!(first, 2); // after the TIDAL and ÁLBUMES headers
        // Down across a kind header lands on the next hit, not on the header.
        assert_eq!(next_hit_row(&rows, Some(3), 1), Some(5));
        // Up across headers (Kind + Source) goes back to the previous hit.
        assert_eq!(next_hit_row(&rows, Some(10), -1), Some(7));
        // Ends.
        assert_eq!(next_hit_row(&rows, Some(first), -1), None);
        assert_eq!(next_hit_row(&rows, Some(rows.len() - 1), 1), None);
        // No selection yet → first hit.
        assert_eq!(next_hit_row(&rows, None, 1), Some(first));
    }

    #[test]
    fn unknown_schemes_go_to_other() {
        let (_, rows) = group_hits(vec![track("spotify:track:1"), track("local:track:1")]);
        assert!(matches!(rows[0], SearchRow::Source { label: "LOCAL", .. }));
        assert!(rows.iter().any(|r| matches!(r, SearchRow::Source { label: "OTHER", .. })));
    }

    #[test]
    fn detail_and_cover_keys() {
        let t = SearchHit::Track(Track {
            uri: "tidal:track:1:2:3".into(),
            album: Some(Album { uri: Some("tidal:album:2".into()), ..Default::default() }),
            ..Default::default()
        });
        assert_eq!(t.detail_key().as_deref(), Some("tidal:album:2"));
        assert_eq!(t.cover_key().as_deref(), Some("tidal:album:2"));
        // A track without an album still has a cover lookup, but no track list.
        let orphan = track("local:track:x");
        assert_eq!(orphan.detail_key(), None);
        assert_eq!(orphan.cover_key().as_deref(), Some("local:track:x"));
        // Artists: cover by their own URI, no track list.
        let a = artist("tidal:artist:9");
        assert_eq!(a.detail_key(), None);
        assert_eq!(a.cover_key().as_deref(), Some("tidal:artist:9"));
        assert_eq!(album("tidal:album:1").detail_key().as_deref(), Some("tidal:album:1"));
    }
}
