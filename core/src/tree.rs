//! The pool's note-commitment tree: Orchard's Sinsemilla Merkle tree, depth 32, kept in memory.
//!
//! Every accepted bundle appends **all** of its actions' `cmx` (dummy outputs included, exactly
//! as Zcash does) and closes with a checkpoint. The sequence of checkpoint roots is the pool's
//! anchor history; a bundle is accepted only against an anchor in it (REQ §3).

use incrementalmerkletree::{Marking, Position, Retention};
use orchard::tree::{MerkleHashOrchard, MerklePath};
use orchard::Anchor;
use shardtree::{store::memory::MemoryShardStore, ShardTree};

use crate::error::Error;

/// Orchard's tree depth.
pub const DEPTH: u8 = 32;
const SHARD_HEIGHT: u8 = 16;
/// Checkpoints retained inside the shard tree. Witnesses are always taken at the latest
/// checkpoint, so this only bounds memory; the anchor history itself is kept separately.
const MAX_CHECKPOINTS: usize = 1 << 16;

type Store = MemoryShardStore<MerkleHashOrchard, u32>;

pub struct CommitmentTree {
    inner: ShardTree<Store, DEPTH, SHARD_HEIGHT>,
    leaf_count: u64,
    checkpoint: u32,
    /// Every root the pool has ever had, oldest first; `anchors[0]` is the empty tree.
    anchors: Vec<[u8; 32]>,
}

impl Default for CommitmentTree {
    fn default() -> Self {
        Self::new()
    }
}

impl CommitmentTree {
    pub fn new() -> Self {
        Self {
            inner: ShardTree::new(MemoryShardStore::empty(), MAX_CHECKPOINTS),
            leaf_count: 0,
            checkpoint: 0,
            anchors: vec![Anchor::empty_tree().to_bytes()],
        }
    }

    pub fn leaf_count(&self) -> u64 {
        self.leaf_count
    }

    /// Number of accepted batches (one checkpoint each).
    pub fn checkpoint_count(&self) -> u32 {
        self.checkpoint
    }

    /// The current root, i.e. the anchor new bundles must be built against.
    pub fn current_anchor(&self) -> Anchor {
        Anchor::from_bytes(*self.anchors.last().expect("history is never empty"))
            .expect("stored anchors are canonical")
    }

    /// Whether `anchor` is a root this pool has had at some point.
    pub fn is_known_anchor(&self, anchor: &Anchor) -> bool {
        self.anchors.contains(&anchor.to_bytes())
    }

    /// The full anchor history, oldest first.
    pub fn anchors(&self) -> &[[u8; 32]] {
        &self.anchors
    }

    /// Appends one bundle's leaves (in action order), marks them all as witnessable and closes
    /// a checkpoint. Returns the new root. Empty batches are refused.
    pub fn append_batch(&mut self, leaves: &[MerkleHashOrchard]) -> Result<Anchor, Error> {
        if leaves.is_empty() {
            return Err(Error::Tree("empty batch".into()));
        }
        let next_checkpoint = self.checkpoint + 1;
        for (i, leaf) in leaves.iter().enumerate() {
            let retention = if i + 1 == leaves.len() {
                Retention::Checkpoint {
                    id: next_checkpoint,
                    marking: Marking::Marked,
                }
            } else {
                Retention::Marked
            };
            self.inner.append(*leaf, retention)?;
        }
        self.checkpoint = next_checkpoint;
        self.leaf_count += leaves.len() as u64;
        let root = self
            .inner
            .root_at_checkpoint_id(&self.checkpoint)?
            .ok_or(Error::Internal("checkpoint has no root"))?;
        let anchor = Anchor::from(root);
        self.anchors.push(anchor.to_bytes());
        Ok(anchor)
    }

    /// Merkle witness for the leaf at `position`, valid under [`Self::current_anchor`].
    pub fn witness(&self, position: u64) -> Result<MerklePath, Error> {
        if position >= self.leaf_count {
            return Err(Error::NoWitness(position));
        }
        let path = self
            .inner
            .witness_at_checkpoint_id(Position::from(position), &self.checkpoint)?
            .ok_or(Error::NoWitness(position))?;
        Ok(MerklePath::from(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use incrementalmerkletree::Hashable;
    use orchard::note::ExtractedNoteCommitment;

    fn leaf(i: u8) -> (MerkleHashOrchard, ExtractedNoteCommitment) {
        // Any canonical field element works as a stand-in commitment.
        let mut b = [0u8; 32];
        b[0] = i + 1;
        let cmx = ExtractedNoteCommitment::from_bytes(&b).unwrap();
        (MerkleHashOrchard::from_cmx(&cmx), cmx)
    }

    #[test]
    fn roots_and_witnesses_are_consistent() {
        let mut t = CommitmentTree::default();
        assert_eq!(t.checkpoint_count(), 0);
        assert_eq!(t.current_anchor(), Anchor::empty_tree());
        assert_eq!(
            t.current_anchor(),
            Anchor::from(MerkleHashOrchard::empty_root(DEPTH.into()))
        );

        let batch1: Vec<_> = (0..2).map(leaf).collect();
        let a1 = t
            .append_batch(&batch1.iter().map(|l| l.0).collect::<Vec<_>>())
            .unwrap();
        let batch2: Vec<_> = (2..5).map(leaf).collect();
        let a2 = t
            .append_batch(&batch2.iter().map(|l| l.0).collect::<Vec<_>>())
            .unwrap();
        assert_ne!(a1, a2);
        assert_eq!(t.leaf_count(), 5);
        assert_eq!(t.checkpoint_count(), 2);
        assert_eq!(t.anchors().len(), 3);
        assert!(t.is_known_anchor(&a1) && t.is_known_anchor(&a2));
        assert!(t.is_known_anchor(&Anchor::empty_tree()));

        // Every leaf witnesses to the current root, including ones from the older batch.
        for (pos, (_, cmx)) in batch1.iter().chain(batch2.iter()).enumerate() {
            let path = t.witness(pos as u64).unwrap();
            assert_eq!(path.position(), pos as u32);
            assert_eq!(path.root(*cmx), a2);
        }
        assert!(matches!(t.witness(5), Err(Error::NoWitness(5))));
        assert!(t.append_batch(&[]).is_err());
    }
}
