use std::path::{Path, PathBuf};

/// Recursively find `.band` project bundles under `root`, excluding `claimed`.
pub fn discover_band_projects(root: &Path, claimed: &Path) -> Vec<PathBuf> {
    let claimed = claimed
        .canonicalize()
        .unwrap_or_else(|_| claimed.to_path_buf());
    let mut found = Vec::new();
    discover_band_projects_inner(root, &claimed, &mut found);
    found.sort();
    found.dedup();
    found
}

fn discover_band_projects_inner(dir: &Path, claimed: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.extension().and_then(|s| s.to_str()) == Some("band") {
                let canonical = path.canonicalize().unwrap_or(path);
                if canonical != *claimed {
                    out.push(canonical);
                }
            } else {
                discover_band_projects_inner(&path, claimed, out);
            }
        }
    }
}
