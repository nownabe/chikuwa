use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct GitInfo {
    pub branch: Option<String>,
    pub pr: Option<PrInfo>,
    pub repo_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PrInfo {
    pub number: u32,
    pub title: String,
}

struct CacheEntry {
    git_info: GitInfo,
    branch_fetched_at: Instant,
    pr_fetched_at: Instant,
    repo_name_fetched: bool,
}

const BRANCH_TTL_SECS: u64 = 2;
const PR_TTL_SECS: u64 = 60;

pub struct GitInfoCache {
    entries: HashMap<PathBuf, CacheEntry>,
}

impl GitInfoCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Get git info for a path, using cached values when fresh enough.
    pub async fn get(&mut self, path: &str) -> Option<GitInfo> {
        let path_buf = PathBuf::from(path);
        let now = Instant::now();

        if let Some(entry) = self.entries.get_mut(&path_buf) {
            // Refresh branch if stale
            if now.duration_since(entry.branch_fetched_at).as_secs() >= BRANCH_TTL_SECS {
                entry.git_info.branch = fetch_branch(path).await;
                entry.branch_fetched_at = now;
            }

            // Refresh PR if stale (and we have a branch)
            if now.duration_since(entry.pr_fetched_at).as_secs() >= PR_TTL_SECS {
                if let Some(ref branch) = entry.git_info.branch {
                    entry.git_info.pr = fetch_pr(path, branch).await;
                } else {
                    entry.git_info.pr = None;
                }
                entry.pr_fetched_at = now;
            }

            // Repo name is fetched once and cached
            if !entry.repo_name_fetched {
                entry.git_info.repo_name = fetch_repo_name(path).await;
                entry.repo_name_fetched = true;
            }

            return Some(entry.git_info.clone());
        }

        // No cache entry — fetch fresh
        let branch = fetch_branch(path).await;
        let pr = if let Some(ref b) = branch {
            fetch_pr(path, b).await
        } else {
            None
        };
        let repo_name = fetch_repo_name(path).await;

        let git_info = GitInfo {
            branch,
            pr,
            repo_name,
        };
        self.entries.insert(
            path_buf,
            CacheEntry {
                git_info: git_info.clone(),
                branch_fetched_at: now,
                pr_fetched_at: now,
                repo_name_fetched: true,
            },
        );

        Some(git_info)
    }

    /// Remove entries for paths no longer active.
    pub fn retain_paths(&mut self, active: &HashSet<PathBuf>) {
        self.entries.retain(|k, _| active.contains(k));
    }
}

/// Get current branch name via `git rev-parse --abbrev-ref HEAD`.
/// Falls back to short SHA for detached HEAD.
async fn fetch_branch(path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        return None;
    }

    // Detached HEAD returns "HEAD"
    if branch == "HEAD" {
        return fetch_short_sha(path).await;
    }

    Some(branch)
}

/// Get short SHA for detached HEAD.
async fn fetch_short_sha(path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(path)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}

/// Get repo name via `git remote get-url origin`, parsed to `owner/repo`.
async fn fetch_repo_name(path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(path)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_repo_name(&url)
}

fn parse_repo_name(url: &str) -> Option<String> {
    let url = url.trim_end_matches(".git");

    // SSH: git@github.com:owner/repo
    if let Some(rest) = url.strip_prefix("git@") {
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if parts.len() == 2 && !parts[1].is_empty() {
            return Some(parts[1].to_string());
        }
    }

    // HTTPS: https://github.com/owner/repo
    if let Some(idx) = url.find("://") {
        let after_scheme = &url[idx + 3..];
        if let Some(slash_idx) = after_scheme.find('/') {
            let repo_path = &after_scheme[slash_idx + 1..];
            if !repo_path.is_empty() {
                return Some(repo_path.to_string());
            }
        }
    }

    None
}

/// Get PR info via `gh pr view <branch> --json number,title`.
async fn fetch_pr(path: &str, branch: &str) -> Option<PrInfo> {
    let output = Command::new("gh")
        .args(["pr", "view", branch, "--json", "number,title"])
        .current_dir(path)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let number = json.get("number")?.as_u64()? as u32;
    let title = json.get("title")?.as_str()?.to_string();

    Some(PrInfo { number, title })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_info_cache_new() {
        let cache = GitInfoCache::new();
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn test_retain_paths_removes_stale() {
        let mut cache = GitInfoCache::new();
        let path1 = PathBuf::from("/home/user/project1");
        let path2 = PathBuf::from("/home/user/project2");

        cache.entries.insert(
            path1.clone(),
            CacheEntry {
                git_info: GitInfo {
                    branch: Some("main".to_string()),
                    pr: None,
                    repo_name: Some("owner/repo1".to_string()),
                },
                branch_fetched_at: Instant::now(),
                pr_fetched_at: Instant::now(),
                repo_name_fetched: true,
            },
        );
        cache.entries.insert(
            path2.clone(),
            CacheEntry {
                git_info: GitInfo {
                    branch: Some("dev".to_string()),
                    pr: None,
                    repo_name: None,
                },
                branch_fetched_at: Instant::now(),
                pr_fetched_at: Instant::now(),
                repo_name_fetched: true,
            },
        );

        let mut active = HashSet::new();
        active.insert(path1.clone());

        cache.retain_paths(&active);

        assert_eq!(cache.entries.len(), 1);
        assert!(cache.entries.contains_key(&path1));
        assert!(!cache.entries.contains_key(&path2));
    }

    #[test]
    fn test_git_info_clone() {
        let info = GitInfo {
            branch: Some("feature/x".to_string()),
            pr: Some(PrInfo {
                number: 42,
                title: "Fix IPC".to_string(),
            }),
            repo_name: Some("nownabe/chikuwa".to_string()),
        };
        let cloned = info.clone();
        assert_eq!(cloned.branch, Some("feature/x".to_string()));
        assert_eq!(cloned.pr.as_ref().unwrap().number, 42);
        assert_eq!(cloned.pr.as_ref().unwrap().title, "Fix IPC");
        assert_eq!(cloned.repo_name, Some("nownabe/chikuwa".to_string()));
    }

    #[test]
    fn test_parse_repo_name_ssh() {
        assert_eq!(
            parse_repo_name("git@github.com:nownabe/chikuwa.git"),
            Some("nownabe/chikuwa".to_string())
        );
        assert_eq!(
            parse_repo_name("git@github.com:owner/repo"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_repo_name_https() {
        assert_eq!(
            parse_repo_name("https://github.com/nownabe/chikuwa.git"),
            Some("nownabe/chikuwa".to_string())
        );
        assert_eq!(
            parse_repo_name("https://github.com/owner/repo"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_repo_name_invalid() {
        assert_eq!(parse_repo_name("not-a-url"), None);
        assert_eq!(parse_repo_name(""), None);
    }
}
