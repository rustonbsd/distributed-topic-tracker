//! Topic management: bootstrap, publishing, and peer merging.

mod bootstrap;
mod publisher;
#[allow(clippy::module_inception)]
mod topic;

pub use bootstrap::Bootstrap;
pub use publisher::Publisher;
pub use topic::{Topic, TopicId};
