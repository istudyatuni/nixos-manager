mod branches;
mod command;
mod download;
mod remote;
mod worktree;

pub use branches::{
    Branch, RemoteBranchesError, RemoteRefs, branch_names, read_branches, remote_branches,
    remote_head,
};
pub use command::available;
pub use download::download;
pub use remote::{CREDENTIAL_HELPER_FLAG, write_credential};
pub use worktree::{WorkingCopy, check_worktree, inspect};

#[derive(Clone, Debug)]
pub enum Error {
    Unavailable,
    Path,
    NotRepository,
    Origin,
    LocalChanges,
    Inspect(String),
    Clone(String),
    Fetch(String),
    Checkout(String),
    Revision,
}
