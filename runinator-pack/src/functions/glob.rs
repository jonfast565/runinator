//! the small path matcher the archive's exclude list uses.
//!
//! a dependency-free subset of the usual glob vocabulary, which is all an exclude list needs:
//! `*` matches within one path segment, `?` matches one character, `**` matches any number of
//! segments, and a pattern ending in `/` excludes a directory and everything under it.

/// true when `path` (a `/`-separated relative path) matches `pattern`.
pub fn matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim_start_matches("./");
    // a trailing slash names a directory, so it matches that directory and everything beneath it:
    // the pattern has to match some *prefix* of the path rather than the whole of it.
    if let Some(directory) = pattern.strip_suffix('/') {
        let directory = split(directory);
        let segments = split(path);
        return (0..=segments.len()).any(|end| matches_segments(&directory, &segments[..end]));
    }
    matches_segments(&split(pattern), &split(path))
}

fn split(value: &str) -> Vec<&str> {
    value.split('/').filter(|part| !part.is_empty()).collect()
}

// walks pattern and path segments together; `**` recurses over how many segments it consumes.
fn matches_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.first() {
        None => path.is_empty(),
        Some(&"**") => {
            // `**` matches zero or more segments, so try every split point.
            (0..=path.len()).any(|taken| matches_segments(&pattern[1..], &path[taken..]))
        }
        Some(head) => match path.first() {
            Some(segment) if matches_segment(head, segment) => {
                matches_segments(&pattern[1..], &path[1..])
            }
            _ => false,
        },
    }
}

// `*` and `?` within a single segment, matched over chars so multi-byte names behave.
fn matches_segment(pattern: &str, segment: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let segment: Vec<char> = segment.chars().collect();
    matches_chars(&pattern, &segment)
}

fn matches_chars(pattern: &[char], segment: &[char]) -> bool {
    match pattern.first() {
        None => segment.is_empty(),
        Some('*') => {
            (0..=segment.len()).any(|taken| matches_chars(&pattern[1..], &segment[taken..]))
        }
        Some('?') => !segment.is_empty() && matches_chars(&pattern[1..], &segment[1..]),
        Some(expected) => {
            segment.first() == Some(expected) && matches_chars(&pattern[1..], &segment[1..])
        }
    }
}
