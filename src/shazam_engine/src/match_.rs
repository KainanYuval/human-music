use crate::fingerprint::Fingerprint;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub score: f32,
    pub aligned: usize,
    pub query_hashes: usize,
    pub offset_s: f32,
    pub peak_votes: usize,
}

pub type FingerprintIndex = HashMap<u64, Vec<f32>>;

pub fn build_index(reference: &[Fingerprint]) -> FingerprintIndex {
    let mut index = FingerprintIndex::new();
    for fp in reference {
        index.entry(fp.hash).or_default().push(fp.time_s);
    }
    index
}

pub fn match_landmarks(query: &[Fingerprint], index: &FingerprintIndex) -> MatchResult {
    if query.is_empty() || index.is_empty() {
        return MatchResult {
            score: 0.0,
            aligned: 0,
            query_hashes: query.len(),
            offset_s: 0.0,
            peak_votes: 0,
        };
    }

    const BIN_HZ: f32 = 20.0;
    const WIN_S: f32 = 0.2;

    let mut votes: HashMap<i32, usize> = HashMap::new();

    for fp in query {
        if let Some(ref_times) = index.get(&fp.hash) {
            for &t_r in ref_times {
                let b = ((t_r - fp.time_s) * BIN_HZ).round() as i32;
                *votes.entry(b).or_default() += 1;
            }
        }
    }

    let Some((&best_bin, &peak)) = votes.iter().max_by_key(|&(_, c)| c) else {
        return MatchResult {
            score: 0.0,
            aligned: 0,
            query_hashes: query.len(),
            offset_s: 0.0,
            peak_votes: 0,
        };
    };

    let offset_s = best_bin as f32 / BIN_HZ;
    let win_bins = (WIN_S * BIN_HZ).round() as i32;
    let _ = win_bins;

    let mut aligned = 0usize;
    for fp in query {
        let Some(ref_times) = index.get(&fp.hash) else {
            continue;
        };
        for &t_r in ref_times {
            if ((t_r - fp.time_s) - offset_s).abs() <= WIN_S {
                aligned += 1;
                break;
            }
        }
    }

    MatchResult {
        score: aligned as f32 / query.len() as f32,
        aligned,
        query_hashes: query.len(),
        offset_s,
        peak_votes: peak,
    }
}
