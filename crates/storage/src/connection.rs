use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

mod block;
mod class;
mod ethereum;
mod event;
mod reference;
mod reorg_counter;
mod signature;
mod state_update;
pub(crate) mod transaction;
mod trie;

use pathfinder_common::receipt::Receipt;
use pathfinder_common::state_update::StateUpdateCounts;
// Re-export this so users don't require rusqlite as a direct dep.
pub use rusqlite::TransactionBehavior;

pub use event::KEY_FILTER_LIMIT as EVENT_KEY_FILTER_LIMIT;
pub use event::PAGE_SIZE_LIMIT as EVENT_PAGE_SIZE_LIMIT;
pub use event::{EmittedEvent, EventFilter, EventFilterError, PageOfEvents};

pub(crate) use reorg_counter::ReorgCounter;

use smallvec::SmallVec;
pub use transaction::TransactionStatus;

pub use trie::{Child, Node, StoredNode};

use pathfinder_common::*;
use pathfinder_crypto::Felt;
use pathfinder_ethereum::EthereumStateUpdate;

use pathfinder_common::transaction::Transaction as StarknetTransaction;

use crate::BlockId;

type PooledConnection = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

pub struct Connection {
    connection: PooledConnection,
    bloom_filter_cache: Arc<crate::bloom::Cache>,
}

impl Connection {
    pub(crate) fn new(
        connection: PooledConnection,
        bloom_filter_cache: Arc<crate::bloom::Cache>,
    ) -> Self {
        Self {
            connection,
            bloom_filter_cache,
        }
    }

    pub fn transaction(&mut self) -> anyhow::Result<Transaction<'_>> {
        let tx = self.connection.transaction()?;
        Ok(Transaction {
            transaction: tx,
            bloom_filter_cache: self.bloom_filter_cache.clone(),
        })
    }

    pub fn transaction_with_behavior(
        &mut self,
        behavior: TransactionBehavior,
    ) -> anyhow::Result<Transaction<'_>> {
        let tx = self.connection.transaction_with_behavior(behavior)?;
        Ok(Transaction {
            transaction: tx,
            bloom_filter_cache: self.bloom_filter_cache.clone(),
        })
    }
}

pub struct Transaction<'inner> {
    transaction: rusqlite::Transaction<'inner>,
    bloom_filter_cache: Arc<crate::bloom::Cache>,
}

impl<'inner> Transaction<'inner> {
    // The implementations here are intentionally kept as simple wrappers. This lets the real implementations
    // be kept in separate files with more reasonable LOC counts and easier test oversight.

    #[cfg(test)]
    pub(crate) fn new(tx: rusqlite::Transaction<'inner>) -> Self {
        Self {
            transaction: tx,
            bloom_filter_cache: Arc::new(crate::bloom::Cache::with_size(1)),
        }
    }

    pub fn insert_contract_state_hash(
        &self,
        block_number: BlockNumber,
        contract: ContractAddress,
        state_hash: ContractStateHash,
    ) -> anyhow::Result<()> {
        trie::insert_contract_state_hash(self, block_number, contract, state_hash)
    }

    pub fn contract_state_hash(
        &self,
        block: BlockNumber,
        contract: ContractAddress,
    ) -> anyhow::Result<Option<ContractStateHash>> {
        trie::contract_state_hash(self, block, contract)
    }

    pub fn insert_block_header(&self, header: &BlockHeader) -> anyhow::Result<()> {
        block::insert_block_header(self, header)
    }

    pub fn block_header(&self, block: BlockId) -> anyhow::Result<Option<BlockHeader>> {
        block::block_header(self, block)
    }

    /// Returns the closest ancestor header that is in storage.
    ///
    /// i.e. returns the latest header with number < target.
    pub fn next_ancestor(
        &self,
        block: BlockNumber,
    ) -> anyhow::Result<Option<(BlockNumber, BlockHash)>> {
        block::next_ancestor(self, block)
    }

    /// Searches in reverse chronological order for a block that exists in storage, but whose parent does not.
    ///
    /// Note that target is included in the search.
    pub fn next_ancestor_without_parent(
        &self,
        block: BlockNumber,
    ) -> anyhow::Result<Option<(BlockNumber, BlockHash)>> {
        block::next_ancestor_without_parent(self, block)
    }

    /// Removes all data related to this block.
    ///
    /// This includes block header, block body and state update information.
    pub fn purge_block(&self, block: BlockNumber) -> anyhow::Result<()> {
        block::purge_block(self, block)
    }

    pub fn block_id(&self, block: BlockId) -> anyhow::Result<Option<(BlockNumber, BlockHash)>> {
        block::block_id(self, block)
    }

    pub fn block_hash(&self, block: BlockId) -> anyhow::Result<Option<BlockHash>> {
        block::block_hash(self, block)
    }

    pub fn block_exists(&self, block: BlockId) -> anyhow::Result<bool> {
        block::block_exists(self, block)
    }

    pub fn block_is_l1_accepted(&self, block: BlockId) -> anyhow::Result<bool> {
        block::block_is_l1_accepted(self, block)
    }

    pub fn first_block_without_transactions(&self) -> anyhow::Result<Option<BlockNumber>> {
        block::first_block_without_transactions(self)
    }

    pub fn first_block_without_receipts(&self) -> anyhow::Result<Option<BlockNumber>> {
        block::first_block_without_receipts(self)
    }

    pub fn update_l1_l2_pointer(&self, block: Option<BlockNumber>) -> anyhow::Result<()> {
        reference::update_l1_l2_pointer(self, block)
    }

    pub fn l1_l2_pointer(&self) -> anyhow::Result<Option<BlockNumber>> {
        reference::l1_l2_pointer(self)
    }

    pub fn upsert_l1_state(&self, update: &EthereumStateUpdate) -> anyhow::Result<()> {
        ethereum::upsert_l1_state(self, update)
    }

    pub fn l1_state_at_number(
        &self,
        block: BlockNumber,
    ) -> anyhow::Result<Option<EthereumStateUpdate>> {
        ethereum::l1_state_at_number(self, block)
    }

    pub fn latest_l1_state(&self) -> anyhow::Result<Option<EthereumStateUpdate>> {
        ethereum::latest_l1_state(self)
    }

    /// Inserts the transaction, receipt and event data.
    pub fn insert_transaction_data(
        &self,
        block_hash: BlockHash,
        block_number: BlockNumber,
        transaction_data: &[(StarknetTransaction, Option<Receipt>)],
    ) -> anyhow::Result<()> {
        transaction::insert_transactions(self, block_hash, block_number, transaction_data)
    }

    pub fn update_receipt(
        &self,
        block_hash: BlockHash,
        transaction_idx: usize,
        receipt: &Receipt,
    ) -> anyhow::Result<()> {
        transaction::update_receipt(self, block_hash, transaction_idx, receipt)
    }

    pub fn transaction_block_hash(
        &self,
        hash: TransactionHash,
    ) -> anyhow::Result<Option<BlockHash>> {
        transaction::transaction_block_hash(self, hash)
    }

    pub fn transaction(
        &self,
        hash: TransactionHash,
    ) -> anyhow::Result<Option<StarknetTransaction>> {
        transaction::transaction(self, hash)
    }

    pub fn transaction_with_receipt(
        &self,
        hash: TransactionHash,
    ) -> anyhow::Result<Option<(StarknetTransaction, Receipt, BlockHash)>> {
        transaction::transaction_with_receipt(self, hash)
    }

    pub fn transaction_at_block(
        &self,
        block: BlockId,
        index: usize,
    ) -> anyhow::Result<Option<StarknetTransaction>> {
        transaction::transaction_at_block(self, block, index)
    }

    pub fn transaction_data_for_block(
        &self,
        block: BlockId,
    ) -> anyhow::Result<Option<Vec<(StarknetTransaction, Receipt)>>> {
        transaction::transaction_data_for_block(self, block)
    }

    pub fn transactions_for_block(
        &self,
        block: BlockId,
    ) -> anyhow::Result<Option<Vec<StarknetTransaction>>> {
        transaction::transactions_for_block(self, block)
    }

    pub fn receipts_for_block(&self, block: BlockId) -> anyhow::Result<Option<Vec<Receipt>>> {
        transaction::receipts_for_block(self, block)
    }

    pub fn transaction_hashes_for_block(
        &self,
        block: BlockId,
    ) -> anyhow::Result<Option<Vec<TransactionHash>>> {
        transaction::transaction_hashes_for_block(self, block)
    }

    pub fn transaction_count(&self, block: BlockId) -> anyhow::Result<usize> {
        transaction::transaction_count(self, block)
    }

    pub fn events(
        &self,
        filter: &EventFilter,
        max_blocks_to_scan: NonZeroUsize,
        max_uncached_bloom_filters_to_load: NonZeroUsize,
    ) -> Result<PageOfEvents, EventFilterError> {
        event::get_events(
            self,
            filter,
            max_blocks_to_scan,
            max_uncached_bloom_filters_to_load,
        )
    }

    pub fn insert_sierra_class(
        &self,
        sierra_hash: &SierraHash,
        sierra_definition: &[u8],
        casm_hash: &CasmHash,
        casm_definition: &[u8],
    ) -> anyhow::Result<()> {
        class::insert_sierra_class(
            self,
            sierra_hash,
            sierra_definition,
            casm_hash,
            casm_definition,
        )
    }

    // TODO: create a CairoHash if sensible instead.
    pub fn insert_cairo_class(
        &self,
        cairo_hash: ClassHash,
        definition: &[u8],
    ) -> anyhow::Result<()> {
        class::insert_cairo_class(self, cairo_hash, definition)
    }

    pub fn insert_class_commitment_leaf(
        &self,
        block: BlockNumber,
        leaf: &ClassCommitmentLeafHash,
        casm_hash: &CasmHash,
    ) -> anyhow::Result<()> {
        class::insert_class_commitment_leaf(self, block, leaf, casm_hash)
    }

    pub fn class_commitment_leaf(
        &self,
        block: BlockNumber,
        casm_hash: &CasmHash,
    ) -> anyhow::Result<Option<ClassCommitmentLeafHash>> {
        class::class_commitment_leaf(self, block, casm_hash)
    }

    /// Returns whether the Sierra or Cairo class definition exists in the database.
    ///
    /// Note that this does not indicate that the class is actually declared -- only that we stored it.
    pub fn class_definitions_exist(&self, classes: &[ClassHash]) -> anyhow::Result<Vec<bool>> {
        class::classes_exist(self, classes)
    }

    /// Returns the uncompressed class definition.
    pub fn class_definition(&self, class_hash: ClassHash) -> anyhow::Result<Option<Vec<u8>>> {
        class::class_definition(self, class_hash)
    }

    /// Returns the uncompressed class definition as well as the block number at which it was declared.
    pub fn class_definition_with_block_number(
        &self,
        class_hash: ClassHash,
    ) -> anyhow::Result<Option<(Option<BlockNumber>, Vec<u8>)>> {
        class::class_definition_with_block_number(self, class_hash)
    }

    /// Returns the compressed class definition if it has been declared at `block_id`.
    pub fn compressed_class_definition_at(
        &self,
        block_id: BlockId,
        class_hash: ClassHash,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        class::compressed_class_definition_at(self, block_id, class_hash)
    }

    /// Returns the uncompressed class definition if it has been declared at `block_id`.
    pub fn class_definition_at(
        &self,
        block_id: BlockId,
        class_hash: ClassHash,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        class::class_definition_at(self, block_id, class_hash)
    }

    /// Returns the uncompressed class definition if it has been declared at `block_id`, as well as
    /// the block number at which it was declared.
    pub fn class_definition_at_with_block_number(
        &self,
        block_id: BlockId,
        class_hash: ClassHash,
    ) -> anyhow::Result<Option<(BlockNumber, Vec<u8>)>> {
        class::class_definition_at_with_block_number(self, block_id, class_hash)
    }

    /// Returns the uncompressed compiled class definition.
    pub fn casm_definition(&self, class_hash: ClassHash) -> anyhow::Result<Option<Vec<u8>>> {
        class::casm_definition(self, class_hash)
    }

    /// Returns the uncompressed compiled class definition, as well as the block number at which it
    ///  was declared.
    pub fn casm_definition_with_block_number(
        &self,
        class_hash: ClassHash,
    ) -> anyhow::Result<Option<(Option<BlockNumber>, Vec<u8>)>> {
        class::casm_definition_with_block_number(self, class_hash)
    }

    /// Returns the uncompressed compiled class definition if it has been declared at `block_id`.
    pub fn casm_definition_at(
        &self,
        block_id: BlockId,
        class_hash: ClassHash,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        class::casm_definition_at(self, block_id, class_hash)
    }

    /// Returns the uncompressed compiled class definition if it has been declared at `block_id`, as well
    /// as the block number at which it was declared.
    pub fn casm_definition_at_with_block_number(
        &self,
        block_id: BlockId,
        class_hash: ClassHash,
    ) -> anyhow::Result<Option<(Option<BlockNumber>, Vec<u8>)>> {
        class::casm_definition_at_with_block_number(self, block_id, class_hash)
    }

    /// Returns hashes of Cairo and Sierra classes declared at a given block.
    pub fn declared_classes_at(&self, block: BlockId) -> anyhow::Result<Option<Vec<ClassHash>>> {
        state_update::declared_classes_at(self, block)
    }

    pub fn contract_class_hash(
        &self,
        block_id: BlockId,
        contract_address: ContractAddress,
    ) -> anyhow::Result<Option<ClassHash>> {
        state_update::contract_class_hash(self, block_id, contract_address)
    }

    /// Returns the compiled class hash for a class.
    pub fn casm_hash(&self, class_hash: ClassHash) -> anyhow::Result<Option<CasmHash>> {
        class::casm_hash(self, class_hash)
    }

    /// Returns the compiled class hash for a class if it has been declared at `block_id`.
    pub fn casm_hash_at(
        &self,
        block_id: BlockId,
        class_hash: ClassHash,
    ) -> anyhow::Result<Option<CasmHash>> {
        class::casm_hash_at(self, block_id, class_hash)
    }

    /// Stores the class trie information.
    pub fn insert_class_trie(
        &self,
        root: ClassCommitment,
        nodes: &HashMap<Felt, Node>,
    ) -> anyhow::Result<u64> {
        trie::trie_class::insert(self, root.0, nodes)
    }

    /// Stores a single contract's storage trie information.
    pub fn insert_contract_trie(
        &self,
        root: ContractRoot,
        nodes: &HashMap<Felt, Node>,
    ) -> anyhow::Result<u64> {
        trie::trie_contracts::insert(self, root.0, nodes)
    }

    /// Stores the global starknet storage trie information.
    pub fn insert_storage_trie(
        &self,
        root: StorageCommitment,
        nodes: &HashMap<Felt, Node>,
    ) -> anyhow::Result<u64> {
        trie::trie_storage::insert(self, root.0, nodes)
    }

    pub fn class_trie_node(&self, index: u64) -> anyhow::Result<Option<StoredNode>> {
        trie::trie_class::node(self, index)
    }

    pub fn storage_trie_node(&self, index: u64) -> anyhow::Result<Option<StoredNode>> {
        trie::trie_storage::node(self, index)
    }

    pub fn contract_trie_node(&self, index: u64) -> anyhow::Result<Option<StoredNode>> {
        trie::trie_contracts::node(self, index)
    }

    pub fn class_trie_node_hash(&self, index: u64) -> anyhow::Result<Option<Felt>> {
        trie::trie_class::hash(self, index)
    }

    pub fn storage_trie_node_hash(&self, index: u64) -> anyhow::Result<Option<Felt>> {
        trie::trie_storage::hash(self, index)
    }

    pub fn contract_trie_node_hash(&self, index: u64) -> anyhow::Result<Option<Felt>> {
        trie::trie_contracts::hash(self, index)
    }

    pub fn class_root_index(&self, block: BlockNumber) -> anyhow::Result<Option<u64>> {
        trie::class_root_index(self, block)
    }

    pub fn storage_root_index(&self, block: BlockNumber) -> anyhow::Result<Option<u64>> {
        trie::storage_root_index(self, block)
    }

    pub fn contract_root_index(
        &self,
        block: BlockNumber,
        contract: ContractAddress,
    ) -> anyhow::Result<Option<u64>> {
        trie::contract_root_index(self, block, contract)
    }

    pub fn contract_root(
        &self,
        block: BlockNumber,
        contract: ContractAddress,
    ) -> anyhow::Result<Option<ContractRoot>> {
        trie::contract_root(self, block, contract)
    }

    pub fn insert_class_root(
        &self,
        block_number: BlockNumber,
        root: Option<u64>,
    ) -> anyhow::Result<()> {
        trie::insert_class_root(self, block_number, root)
    }

    pub fn insert_storage_root(
        &self,
        block_number: BlockNumber,
        root: Option<u64>,
    ) -> anyhow::Result<()> {
        trie::insert_storage_root(self, block_number, root)
    }

    pub fn insert_contract_root(
        &self,
        block_number: BlockNumber,
        contract: ContractAddress,
        root: Option<u64>,
    ) -> anyhow::Result<()> {
        trie::insert_contract_root(self, block_number, contract, root)
    }

    pub fn insert_state_update(
        &self,
        block_number: BlockNumber,
        state_update: &StateUpdate,
    ) -> anyhow::Result<()> {
        state_update::insert_state_update(self, block_number, state_update)
    }

    pub fn insert_state_update_counts(
        &self,
        block_number: BlockNumber,
        counts: &StateUpdateCounts,
    ) -> anyhow::Result<()> {
        state_update::update_state_update_counts(self, block_number, counts)
    }

    pub fn state_update(&self, block: BlockId) -> anyhow::Result<Option<StateUpdate>> {
        state_update::state_update(self, block)
    }

    pub fn highest_block_with_state_update(&self) -> anyhow::Result<Option<BlockNumber>> {
        state_update::highest_block_with_state_update(self)
    }

    /// Items are sorted in descending order.
    pub fn state_update_counts(
        &self,
        block: BlockId,
        max_len: NonZeroUsize,
    ) -> anyhow::Result<SmallVec<[StateUpdateCounts; 10]>> {
        state_update::state_update_counts(self, block, max_len)
    }

    pub fn storage_value(
        &self,
        block: BlockId,
        contract_address: ContractAddress,
        key: StorageAddress,
    ) -> anyhow::Result<Option<StorageValue>> {
        state_update::storage_value(self, block, contract_address, key)
    }

    pub fn contract_nonce(
        &self,
        contract_address: ContractAddress,
        block_id: BlockId,
    ) -> anyhow::Result<Option<ContractNonce>> {
        state_update::contract_nonce(self, contract_address, block_id)
    }

    pub fn contract_exists(
        &self,
        contract_address: ContractAddress,
        block_id: BlockId,
    ) -> anyhow::Result<bool> {
        state_update::contract_exists(self, contract_address, block_id)
    }

    pub fn insert_signature(
        &self,
        block_number: BlockNumber,
        signature: &BlockCommitmentSignature,
    ) -> anyhow::Result<()> {
        signature::insert_signature(self, block_number, signature)
    }

    pub fn signature(&self, block: BlockId) -> anyhow::Result<Option<BlockCommitmentSignature>> {
        signature::signature(self, block)
    }

    pub fn increment_reorg_counter(&self) -> anyhow::Result<()> {
        reorg_counter::increment_reorg_counter(self)
    }

    fn reorg_counter(&self) -> anyhow::Result<ReorgCounter> {
        reorg_counter::reorg_counter(self)
    }

    pub(self) fn inner(&self) -> &rusqlite::Transaction<'_> {
        &self.transaction
    }

    pub fn commit(self) -> anyhow::Result<()> {
        Ok(self.transaction.commit()?)
    }
}
