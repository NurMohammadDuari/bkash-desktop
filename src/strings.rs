//! Printable-run string extraction, the same lower-bound method REA uses for
//! the snapshot string pool: byte-pattern runs, not deserialized cluster
//! semantics. Counts are explicitly lower bounds.

/// Minimum printable run length. 8 keeps Dart interned strings and drops noise.
pub const DEFAULT_MIN_LEN: usize = 8;

fn is_printable(b: u8) -> bool {
    matches!(b, 0x20..=0x7e)
}

/// Collect printable ASCII runs of at least `min_len` bytes, in file order.
///
/// Returns `(strings, truncated)` where `truncated == true` means the `cap`
/// guard fired and the output is an incomplete sample.
pub fn printable_runs(bytes: &[u8], min_len: usize, cap: usize) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut truncated = false;

    for (i, &b) in bytes.iter().enumerate() {
        match (is_printable(b), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s >= min_len {
                    if out.len() == cap {
                        truncated = true;
                        return (out, truncated);
                    }
                    out.push(String::from_utf8_lossy(&bytes[s..i]).into_owned());
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        let end = bytes.len();
        if end - s >= min_len && out.len() < cap {
            out.push(String::from_utf8_lossy(&bytes[s..end]).into_owned());
        } else if end - s >= min_len {
            truncated = true;
        }
    }
    (out, truncated)
}

/// Filter runs down to absolute http/https URLs, deduplicated and sorted.
pub fn urls(runs: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for s in runs {
        let mut it = s.match_indices("http");
        while let Some(pos) = it.next().map(|(p, _)| p) {
            let tail = &s[pos..];
            let end = tail
                .find(|c: char| {
                    c.is_whitespace()
                        || matches!(c, '"' | '\'' | '<' | '>' | '[' | '\\' | ')' | ']' | '{' | '}')
                })
                .unwrap_or(tail.len());
            let candidate = tail[..end].trim_end_matches(['.', ',', ')', ']', ';']);
            if candidate.starts_with("http://") || candidate.starts_with("https://") {
                if candidate.len() > 12 {
                    seen.insert(candidate.to_string());
                }
            }
        }
    }
    seen.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_runs_and_respects_min_len() {
        let hay = b"\x00\x00short\x00https://api.cde.capp.bka.sh/api/\x00\x01ab";
        let (runs, truncated) = printable_runs(hay, 8, 100);
        assert!(!truncated);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0], "https://api.cde.capp.bka.sh/api/");
    }

    #[test]
    fn cap_marks_results_as_truncated() {
        let mut hay = Vec::new();
        for i in 0..10 {
            hay.extend_from_slice(format!("entry{i:08}\n").as_bytes());
        }
        let (runs, truncated) = printable_runs(&hay, 8, 3);
        assert!(truncated);
        assert_eq!(runs.len(), 3);
    }

    #[test]
    fn urls_dedupe_and_sort_and_trim_punctuation() {
        let runs = vec![
            "see https://b.bka.sh/x and https://a.bka.sh/y.".to_string(),
            "again https://b.bka.sh/x".to_string(),
            "no url here".to_string(),
        ];
        assert_eq!(
            urls(&runs),
            vec!["https://a.bka.sh/y", "https://b.bka.sh/x"]
        );
    }

    #[test]
    fn plain_http_is_kept_and_junk_is_not() {
        let runs = vec!["http://zc-face.assets.alipay.net/x.html".to_string(), "x".to_string()];
        assert_eq!(urls(&runs).len(), 1);
    }
}