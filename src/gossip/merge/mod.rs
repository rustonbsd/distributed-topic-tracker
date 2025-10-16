mod bubble;
mod message_overlap;

pub use bubble::BubbleMerge;
pub use message_overlap::MessageOverlapMerge;

pub const MAX_JOIN_PEERS_COUNT: usize = 30;
