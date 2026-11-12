//! GitHub API client for loading public repositories.
//! Currently a stub.

/// Metadata for a GitHub repository.
#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub owner: String,
    pub name: String,
    pub default_branch: String,
    pub file_count: usize,
}

/// Fetch repository file tree from GitHub API.
/// Stub: returns None.
pub fn fetch_repo_tree(_owner: &str, _repo: &str) -> Option<Vec<String>> {
    // TODO: Implement with reqwest
    // - GET https://api.github.com/repos/{owner}/{repo}/git/trees/{branch}?recursive=1
    // - Parse response into file path list
    None
}

/// Fetch a single file's content from GitHub.
/// Stub: returns None.
pub fn fetch_file_content(_owner: &str, _repo: &str, _path: &str) -> Option<String> {
    // TODO: GET https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}
    None
}
