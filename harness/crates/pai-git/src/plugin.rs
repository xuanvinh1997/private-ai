//! Mounts the five read tools onto one repository.
//!
//! Project-layer, like `RagPlugin`: the root arrives at construction from the open project,
//! so switching projects unplugs this and plugs in a new one pointed somewhere else. The
//! consequence worth stating is that there is no repository parameter on any tool — the
//! model cannot ask about a repository the user has not opened, and no amount of creative
//! pathspec writing changes which repository is being read.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_tools::{Overflow, Tools};

use crate::repo::Repo;
use crate::tools::{GitBlame, GitDiff, GitLog, GitShow, GitStatus};

pub struct GitPlugin {
    /// The repository root. Usually the project root; a project that is not a git repository
    /// simply gets tools that answer "không phải là một kho git", which is a better failure
    /// than a missing tool the model then works around with `bash`.
    ///
    /// One limit worth stating plainly, because it is the kind of thing a reader assumes the
    /// other way: this is where git is *run*, not a wall git cannot see past. Git discovers
    /// its repository by walking up, exactly as it does in a terminal, so when the project
    /// root is a subdirectory of a larger repository these tools report that whole
    /// repository — history and diffs of sibling folders included. Pathspecs the model sends
    /// are still confined to this root by [`Repo::relative`]; a call that sends none is not.
    /// Mounting this over a folder whose siblings the agent must not read is therefore a
    /// decision for the caller, not something the crate can take back.
    root: PathBuf,
}

impl GitPlugin {
    pub fn new(root: PathBuf) -> GitPlugin {
        GitPlugin { root }
    }
}

#[async_trait]
impl Plugin for GitPlugin {
    fn name(&self) -> &'static str {
        "git"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let repo = Arc::new(Repo::new(self.root.clone()));
        let tools = ctx.require::<Tools>()?;
        // Built from `ctx`, so the spill store is looked up at call time rather than now;
        // the same reason `FsPlugin` builds it this way.
        let overflow = Overflow::new(ctx);

        ctx.keep(tools.register(Arc::new(GitStatus::new(repo.clone(), overflow.clone()))));
        ctx.keep(tools.register(Arc::new(GitDiff::new(repo.clone(), overflow.clone()))));
        ctx.keep(tools.register(Arc::new(GitLog::new(repo.clone(), overflow.clone()))));
        ctx.keep(tools.register(Arc::new(GitShow::new(repo.clone(), overflow.clone()))));
        ctx.keep(tools.register(Arc::new(GitBlame::new(repo, overflow))));
        Ok(())
    }
}
