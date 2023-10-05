use crate::Repository;
pub use gix_status as plumbing;

/// A structure to hold options configuring the status request, which can then be turned into an iterator.
#[derive(Clone)]
pub struct Platform<'repo> {
    repo: &'repo Repository,
}

/// Status
impl Repository {
    /// Obtain a platform for configuring and traversing the git repository status.
    pub fn status(&self) -> Platform<'_> {
        Platform { repo: self }
    }
}
