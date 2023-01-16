use stark_hash::Felt;

pub mod contract_state;
pub mod merkle_node;
pub mod merkle_tree;
pub mod state_tree;

/// Hashing function used by a particular merkle tree implementation.
pub trait Hash {
    fn hash(left: Felt, right: Felt) -> Felt;
}

/// Implements [Hash] for the [StarkNet Pedersen hash](stark_hash::stark_hash).
#[derive(Debug, Clone, Copy)]
pub struct PedersenHash {}

impl Hash for PedersenHash {
    fn hash(left: Felt, right: Felt) -> Felt {
        stark_hash::stark_hash(left, right)
    }
}

/// Implements [Hash] for the StarkNet Poseidon hash.
///
/// TODO: add once hash is implemented.
struct PoseidonHash;
impl crate::Hash for PoseidonHash {
    fn hash(_left: stark_hash::Felt, _right: stark_hash::Felt) -> stark_hash::Felt {
        unimplemented!("Hash function still needs to be implemented");
    }
}