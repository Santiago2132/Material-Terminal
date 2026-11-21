//! Non-blocking background Git branch detector reading `.git/HEAD`.

use crossbeam_channel::{Receiver, Sender, unbounded};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

/// Resolves the Git branch for `dir` rapidly by walking up ancestor `.git/HEAD` files.
pub fn detect_git_branch_sync(dir: &Path) -> Option<String> {
    let mut cur = Some(dir);
    while let Some(candidate) = cur {
        let dot_git = candidate.join(".git");
        if dot_git.is_dir() {
            return read_git_head(&dot_git.join("HEAD"));
        } else if dot_git.is_file()
            && let Ok(contents) = fs::read_to_string(&dot_git)
            && let Some(gitdir_line) = contents.lines().find(|l| l.starts_with("gitdir:"))
        {
            let rel = gitdir_line.trim_start_matches("gitdir:").trim();
            let gitdir_path = if Path::new(rel).is_absolute() {
                PathBuf::from(rel)
            } else {
                candidate.join(rel)
            };
            return read_git_head(&gitdir_path.join("HEAD"));
        }
        cur = candidate.parent();
    }
    None
}

fn read_git_head(head_path: &Path) -> Option<String> {
    let raw = fs::read_to_string(head_path).ok()?;
    let trimmed = raw.trim();
    if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
        return Some(branch.to_string());
    }
    if let Some(other_ref) = trimmed.strip_prefix("ref: ") {
        return Some(
            other_ref
                .rsplit('/')
                .next()
                .unwrap_or(other_ref)
                .to_string(),
        );
    }
    if trimmed.len() >= 7 {
        Some(format!("@{}", &trimmed[..7]))
    } else {
        None
    }
}

/// Request sent to the background Git branch worker.
#[derive(Debug, Clone)]
pub struct GitBranchRequest {
    pub pane_id: u64,
    pub block_id: Option<u64>,
    pub dir: PathBuf,
}

/// Response sent back asynchronously to the UI thread.
#[derive(Debug, Clone)]
pub struct GitBranchResult {
    pub pane_id: u64,
    pub block_id: Option<u64>,
    pub dir: PathBuf,
    pub branch: Option<String>,
}

/// Non-blocking Git branch detector running on a dedicated worker thread.
pub struct AsyncGitDetector {
    req_tx: Sender<GitBranchRequest>,
    res_rx: Receiver<GitBranchResult>,
}

impl AsyncGitDetector {
    pub fn spawn(wake_cb: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
        let (req_tx, req_rx) = unbounded::<GitBranchRequest>();
        let (res_tx, res_rx) = unbounded::<GitBranchResult>();

        let _ = thread::Builder::new()
            .name("git-branch-detector".to_string())
            .spawn(move || {
                while let Ok(req) = req_rx.recv() {
                    let branch = detect_git_branch_sync(&req.dir);
                    if res_tx
                        .send(GitBranchResult {
                            pane_id: req.pane_id,
                            block_id: req.block_id,
                            dir: req.dir,
                            branch,
                        })
                        .is_err()
                    {
                        break;
                    }
                    if let Some(cb) = &wake_cb {
                        cb();
                    }
                }
            });

        Self { req_tx, res_rx }
    }

    pub fn request(&self, pane_id: u64, block_id: Option<u64>, dir: PathBuf) {
        let _ = self.req_tx.send(GitBranchRequest {
            pane_id,
            block_id,
            dir,
        });
    }

    pub fn poll_results(&self) -> Vec<GitBranchResult> {
        let mut results = Vec::new();
        while let Ok(r) = self.res_rx.try_recv() {
            results.push(r);
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn detects_git_branch_async_in_current_repo() {
        let detector = AsyncGitDetector::spawn(None);
        let cwd = std::env::current_dir().unwrap();
        detector.request(1, None, cwd);

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut got = None;
        while Instant::now() < deadline {
            if let Some(res) = detector.poll_results().into_iter().next() {
                got = Some(res);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let res = got.expect("expected async git branch result");
        assert!(res.branch.is_some());
    }
}
