//! BDK Core - a PoC of a Rust wallet for Bitcoin Core connected over IPC.
//!
//! This is PoC-level code. There is inadequacy and races for instance i did not bother to
//! fix. Most likely bugs, too. But this is enough to showcase a fully-featured wallet which
//! connects to Bitcoin Core over IPC and subscribes to its `Chain` notifications to keep
//! its state up to date. This PoC handles confirmed / unconfirmed transaction tracking. Catchup
//! at startup. Reorgs happening either at runtime or detected at startup. Etc..

use bdk_chain::{
    self,
    bitcoin::{self, consensus::Decodable, hashes::Hash},
    keychain_txout::KeychainTxOutIndex,
    local_chain::LocalChain,
    miniscript::{Descriptor, DescriptorPublicKey},
    BlockId, CheckPoint, ConfirmationBlockTime, IndexedTxGraph, Merge,
};
use bdk_file_store::Store as BdkStore;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};
use tokio::task::{self, JoinHandle};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use std::{
    env, error,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

#[allow(dead_code)]
mod chain_capnp;
mod common_capnp;
mod echo_capnp;
mod handler_capnp;
#[allow(dead_code)]
mod init_capnp;
mod mining_capnp;
#[allow(dead_code)]
mod proxy_capnp;
use chain_capnp::{
    chain::Client as ChainClient,
    chain_notifications::{
        BlockConnectedParams, BlockConnectedResults, BlockDisconnectedParams,
        BlockDisconnectedResults, ChainStateFlushedParams, ChainStateFlushedResults, DestroyParams,
        DestroyResults, TransactionAddedToMempoolParams, TransactionAddedToMempoolResults,
        TransactionRemovedFromMempoolParams, TransactionRemovedFromMempoolResults,
        UpdatedBlockTipParams, UpdatedBlockTipResults,
    },
};
use init_capnp::init::Client as InitClient;
use proxy_capnp::thread::Client as ThreadClient;

// When connected to Bitcoin Core we will sleep for that many seconds before disconnecting
// and exiting the program. Feel free to change.
const SLEEP_BEFORE_DISCONNECT_SECS: u64 = 6;

// I only track a single descriptor (no change) for the purpose of this PoC. Feel free to
// change it and/or the network if you want to try for instance on Signet.
const NETWORK: bitcoin::Network = bitcoin::Network::Regtest;
// xprv9zLMbgyqu9kLGJEpgsZhMZKYsAk4NUmwX7mnGdj3HFD5WYoNbMrmfefhveVB5ts12SyEuZHTHMTy9qHCMiuMF4fx1vDExza3Nocrctcm48s
const DESCRIPTOR: &str = "tr(xpub6DKi1CWjjXJdUnKHnu6hihGHRCaYmwVntLhP528eqak4PM8X8uB2DSzBmuTx6kJcUu2dVFLnkpoFudCYNVFGVoa2G5JLwVD4gSDZtncGjpK/*)";

// Persistence for the BDK wallet state
const BDK_STORE_PATH: &str = "bdk_core_store.dat";
const BDK_STORE_MAGIC: &[u8] = b"bdk_core_store";

#[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct ChangeSet {
    chain_cs: bdk_chain::local_chain::ChangeSet,
    graph_cs: bdk_chain::indexed_tx_graph::ChangeSet<
        ConfirmationBlockTime,
        bdk_chain::indexer::keychain_txout::ChangeSet,
    >,
}

impl Merge for ChangeSet {
    fn merge(&mut self, other: Self) {
        Merge::merge(&mut self.chain_cs, other.chain_cs);
        Merge::merge(&mut self.graph_cs, other.graph_cs);
    }

    fn is_empty(&self) -> bool {
        self.chain_cs.is_empty() && self.graph_cs.is_empty()
    }
}

/// The wallet state. Maintains the BDK transaction graph and chain state.
struct BdkWallet {
    chain: LocalChain,
    tx_graph: IndexedTxGraph<ConfirmationBlockTime, KeychainTxOutIndex<()>>,
    store: BdkStore<ChangeSet>,
}

impl BdkWallet {
    /// Create a fresh wallet or open it if a store is available.
    pub fn new() -> Result<Self, Box<dyn error::Error>> {
        let (mut chain, _) =
            LocalChain::from_genesis_hash(bitcoin::constants::genesis_block(NETWORK).block_hash());
        let mut index = KeychainTxOutIndex::default();
        let desc: Descriptor<DescriptorPublicKey> = Descriptor::from_str(DESCRIPTOR)
            .expect("DESCRIPTOR constant must be a valid descriptor.");
        index
            .insert_descriptor((), desc)
            .expect("First to be inserted");
        let mut tx_graph = IndexedTxGraph::new(index);
        let mut store: BdkStore<ChangeSet> =
            BdkStore::open_or_create_new(BDK_STORE_MAGIC, BDK_STORE_PATH)?;
        for cs in store.iter_changesets() {
            let cs = cs?;
            chain.apply_changeset(&cs.chain_cs)?;
            tx_graph.apply_changeset(cs.graph_cs);
        }
        Ok(Self {
            chain,
            tx_graph,
            store,
        })
    }

    pub fn genesis_hash(&self) -> bitcoin::BlockHash {
        self.chain.genesis_hash()
    }

    pub fn tip(&self) -> BlockId {
        self.chain.tip().block_id()
    }

    /// Apply the effects of a block on the wallet. Persist the changes to disk.
    pub fn apply_block(
        &mut self,
        block: &bitcoin::Block,
        height: i32,
    ) -> Result<(), Box<dyn error::Error>> {
        let h: u32 = height.try_into().expect("Must never be negative");
        let graph_cs = self.tx_graph.apply_block_relevant(block, h);
        let chain_cs = self
            .chain
            .apply_update(CheckPoint::from_header(&block.header, h))?;
        let cs = ChangeSet { graph_cs, chain_cs };
        self.store.append_changeset(&cs)?;
        if !cs.graph_cs.is_empty() {
            println!("Graph change set not empty. Here is the new state of the wallet.");
            self.print_info()?;
        }
        Ok(())
    }

    /// Apply the effects of a transaction on the wallet. Persist the changes to disk.
    pub fn apply_tx(&mut self, tx: bitcoin::Transaction) -> Result<(), Box<dyn error::Error>> {
        let graph_cs = self
            .tx_graph
            .batch_insert_relevant_unconfirmed([(tx, /*TODO*/ 0)]);
        let cs = ChangeSet {
            graph_cs,
            ..Default::default()
        };
        self.store.append_changeset(&cs)?;
        if !cs.graph_cs.is_empty() {
            println!("Graph change set not empty. Here is the new state of the wallet.");
            self.print_info()?;
        }
        Ok(())
    }

    /// Mark a block as disconnected. Persist to disk.
    pub fn disconnect(&mut self, block_id: BlockId) -> Result<(), Box<dyn error::Error>> {
        // FIXME: it should never be necessary.
        let chain_cs = if block_id.height > 0 {
            self.chain
                .disconnect_from(block_id)
                .expect("Just checked it's not the genesis")
        } else {
            let mut cs = bdk_chain::local_chain::ChangeSet::default();
            for cp in self.chain.iter_checkpoints() {
                if cp.height() != 0 {
                    cs.blocks.insert(cp.height(), None);
                }
            }
            self.chain = LocalChain::from_genesis_hash(
                bitcoin::constants::genesis_block(NETWORK).block_hash(),
            )
            .0;
            cs
        };
        self.store.append_changeset(&ChangeSet {
            chain_cs,
            ..Default::default()
        })?;
        Ok(())
    }

    /// The first address that we don't know has been used onchain.
    pub fn next_unused_address(&mut self) -> Result<bitcoin::Address, Box<dyn error::Error>> {
        let ((_, script), cs) = self
            .tx_graph
            .index
            .next_unused_spk(())
            .expect("We assume a ranged descriptor is in use");
        let graph_cs = bdk_chain::indexed_tx_graph::ChangeSet {
            indexer: cs,
            ..Default::default()
        };
        self.store.append_changeset(&ChangeSet {
            graph_cs,
            ..Default::default()
        })?;
        Ok(bitcoin::Address::from_script(&script, &NETWORK)
            .expect("We assume the descriptor type used has defined addresses"))
    }

    /// The first address to have never been revealed by this wallet.
    pub fn next_address(&mut self) -> Result<bitcoin::Address, Box<dyn error::Error>> {
        let ((_, script), cs) = self
            .tx_graph
            .index
            .reveal_next_spk(())
            .expect("We assume a ranged descriptor is in use");
        let graph_cs = bdk_chain::indexed_tx_graph::ChangeSet {
            indexer: cs,
            ..Default::default()
        };
        self.store.append_changeset(&ChangeSet {
            graph_cs,
            ..Default::default()
        })?;
        Ok(bitcoin::Address::from_script(&script, &NETWORK)
            .expect("We assume the descriptor type used has defined addresses"))
    }

    /// Print the wallet state (addresses, coins, transactions, balance, ..).
    pub fn print_info(&mut self) -> Result<(), Box<dyn error::Error>> {
        let next_unused_addr = self.next_unused_address()?;
        let next_addr = self.next_address()?;
        let outpoints = self
            .tx_graph
            .index
            .outpoints()
            .into_iter()
            .map(|((_, _), op)| ((), *op));
        let graph = self.tx_graph.graph();
        let balance = graph.balance(&self.chain, self.tip(), outpoints.clone(), |_, _| true);
        let utxos = graph.filter_chain_unspents(&self.chain, self.tip(), outpoints);
        let txs = graph.full_txs().map(|tx| {
            (
                tx.txid,
                graph.get_chain_position(&self.chain, self.tip(), tx.txid),
            )
        });

        println!("Wallet info:");
        println!("      Next unused address: {}.", next_unused_addr);
        println!("      Next unrevealed address: {}.", next_addr);
        println!(
            "      Balance (confirmed + unconfirmed): {}.",
            balance.trusted_spendable()
        );
        print!("      Utxos: ");
        for (_, utxo) in utxos {
            print!("{} ({}), ", utxo.outpoint, utxo.txout.value);
        }
        print!("\n      Transactions: ");
        for (txid, pos) in txs {
            print!("{} (chain pos: {:?}), ", txid, pos);
        }
        println!();

        Ok(())
    }
}

/// IPC interface to Bitcoin Core.
struct RpcInterface {
    rpc_handle: JoinHandle<Result<(), capnp::Error>>,
    disconnector: capnp_rpc::Disconnector<twoparty::VatId>,
    thread: ThreadClient,
    chain_interface: ChainClient,
}

// In the implementation of methods of the IPC interface i used `unwraps()` for
// brievety but if reused you should probably have proper error handling.
impl RpcInterface {
    /// Create an IPC interface by performing the handshake with Bitcoin Core on the provided stream.
    pub async fn new(stream: tokio::net::UnixStream) -> Result<Self, Box<dyn std::error::Error>> {
        let (reader, writer) = stream.into_split();
        let network = Box::new(twoparty::VatNetwork::new(
            reader.compat(),
            writer.compat_write(),
            rpc_twoparty_capnp::Side::Client,
            Default::default(),
        ));

        let mut rpc = RpcSystem::new(network, None);
        let init_interface: InitClient = rpc.bootstrap(rpc_twoparty_capnp::Side::Server);
        let disconnector = rpc.get_disconnector();
        let rpc_handle = task::spawn_local(rpc);

        let mk_init_req = init_interface.construct_request();
        let response = mk_init_req.send().promise.await?;
        let thread_map = response.get()?.get_thread_map()?;

        let mk_thread_req = thread_map.make_thread_request();
        let response = mk_thread_req.send().promise.await?;
        let thread = response.get()?.get_result()?;

        let mut mk_chain_req = init_interface.make_chain_request();
        mk_chain_req.get().get_context()?.set_thread(thread.clone());
        let response = mk_chain_req.send().promise.await?;
        let chain_interface = response.get()?.get_result()?;

        // Send an init message to Core to exercise the interface.
        let mut mk_mess_req = chain_interface.init_message_request();
        mk_mess_req.get().get_context()?.set_thread(thread.clone());
        mk_mess_req
            .get()
            .set_message("Oxydation of the Bitcoin Core wallet in progress..");
        let _ = mk_mess_req.send().promise.await?;

        Ok(Self {
            rpc_handle,
            thread,
            chain_interface,
            disconnector,
        })
    }

    pub async fn get_tip(&self) -> BlockId {
        let mut height_req = self.chain_interface.get_height_request();
        height_req
            .get()
            .get_context()
            .unwrap()
            .set_thread(self.thread.clone());
        let response = height_req.send().promise.await.unwrap();
        let height_i32 = response.get().unwrap().get_result();
        let height = height_i32.try_into().expect("Height is never negative.");

        let mut hash_req = self.chain_interface.get_block_hash_request();
        hash_req
            .get()
            .get_context()
            .unwrap()
            .set_thread(self.thread.clone());
        hash_req.get().set_height(height_i32);
        let response = hash_req.send().promise.await.unwrap();
        let hash = bitcoin::BlockHash::from_slice(response.get().unwrap().get_result().unwrap())
            .expect("Core must be serving valid hashes.");

        BlockId { height, hash }
    }

    // NOTE: not entirely correct, but good enough for the purpose of this PoC
    pub async fn is_in_best_chain(
        &self,
        node_tip_hash: &bitcoin::BlockHash,
        ancestor: &bitcoin::BlockHash,
    ) -> bool {
        let mut find_req = self.chain_interface.find_ancestor_by_hash_request();
        find_req
            .get()
            .get_context()
            .unwrap()
            .set_thread(self.thread.clone());
        find_req.get().set_block_hash(node_tip_hash.as_ref());
        find_req.get().set_ancestor_hash(ancestor.as_ref());
        let response = find_req.send().promise.await.unwrap();
        response.get().unwrap().get_result()
    }

    pub async fn has_blocks(&self, node_tip_hash: &bitcoin::BlockHash, start_height: i32) -> bool {
        let mut has_blocks_req = self.chain_interface.has_blocks_request();
        has_blocks_req
            .get()
            .get_context()
            .unwrap()
            .set_thread(self.thread.clone());
        has_blocks_req.get().set_block_hash(node_tip_hash.as_ref());
        has_blocks_req.get().set_min_height(start_height);
        let response = has_blocks_req.send().promise.await.unwrap();
        response.get().unwrap().get_result()
    }

    pub async fn get_block(
        &self,
        node_tip_hash: &bitcoin::BlockHash,
        height: i32,
    ) -> bitcoin::Block {
        let mut find_req = self.chain_interface.find_ancestor_by_height_request();
        find_req
            .get()
            .get_context()
            .unwrap()
            .set_thread(self.thread.clone());
        find_req.get().set_block_hash(node_tip_hash.as_ref());
        find_req.get().set_ancestor_height(height);
        find_req.get().get_ancestor().unwrap().set_want_data(true);
        let response = find_req.send().promise.await.unwrap();
        bitcoin::Block::consensus_decode(
            &mut response
                .get()
                .unwrap()
                .get_ancestor()
                .unwrap()
                .get_data()
                .unwrap(),
        )
        .expect("Core must provide valid blocks")
    }

    pub async fn common_ancestor(
        &self,
        node_tip_hash: &bitcoin::BlockHash,
        wallet_tip_hash: &bitcoin::BlockHash,
    ) -> Option<BlockId> {
        let mut find_req = self.chain_interface.find_common_ancestor_request();
        find_req
            .get()
            .get_context()
            .unwrap()
            .set_thread(self.thread.clone());
        find_req.get().set_block_hash1(node_tip_hash.as_ref());
        find_req.get().set_block_hash1(wallet_tip_hash.as_ref());
        find_req.get().get_ancestor().unwrap().set_want_height(true);
        find_req.get().get_ancestor().unwrap().set_want_hash(true);
        let response = find_req.send().promise.await.unwrap();
        let response = response.get().unwrap();
        let ancestor = response.get_ancestor().unwrap();
        if !ancestor.get_found() {
            return None;
        }
        let height = ancestor
            .get_height()
            .try_into()
            .expect("Can't be negative.");
        let hash = bitcoin::BlockHash::from_slice(ancestor.get_hash().unwrap())
            .expect("Core must provide valid blocks");
        Some(BlockId { height, hash })
    }

    pub async fn show_progress(&self, title: &str, progress: i32, resume_possible: bool) {
        let mut mk_mess_req = self.chain_interface.show_progress_request();
        mk_mess_req
            .get()
            .get_context()
            .unwrap()
            .set_thread(self.thread.clone());
        mk_mess_req.get().set_title(title);
        mk_mess_req.get().set_progress(progress);
        mk_mess_req.get().set_resume_possible(resume_possible);
        let _ = mk_mess_req.send().promise.await.unwrap();
    }

    pub async fn register_notifications(&self, wallet: Arc<Mutex<BdkWallet>>) {
        let notif_handler = capnp_rpc::new_client(wallet);
        let mut register_req = self.chain_interface.handle_notifications_request();
        register_req
            .get()
            .get_context()
            .unwrap()
            .set_thread(self.thread.clone());
        register_req.get().set_notifications(notif_handler);
        let _ = register_req.send().promise.await.unwrap();
    }

    pub async fn disconnect(self) -> Result<(), capnp::Error> {
        self.disconnector.await.unwrap();
        self.rpc_handle.await.unwrap()
    }
}

// Implementation of the subscription to validation events from Bitcoin Core. Main logic post startup.
impl chain_capnp::chain_notifications::Server for Arc<Mutex<BdkWallet>> {
    fn destroy(
        &mut self,
        _: DestroyParams,
        _: DestroyResults,
    ) -> ::capnp::capability::Promise<(), ::capnp::Error> {
        unimplemented!("Destroy notification") // TODO: do we ever receive this? Looks like not.
    }

    fn transaction_added_to_mempool(
        &mut self,
        params: TransactionAddedToMempoolParams,
        _: TransactionAddedToMempoolResults,
    ) -> ::capnp::capability::Promise<(), ::capnp::Error> {
        let tx = bitcoin::Transaction::consensus_decode(&mut pry!(pry!(params.get()).get_tx()))
            .expect("Core must provide valid transactions.");
        let txid = tx.compute_txid();
        println!("New mempool transaction {}.", txid);
        if let Err(e) = self.lock().unwrap().apply_tx(tx) {
            eprintln!("Error applying tx {} to wallet: {}", txid, e);
        }
        ::capnp::capability::Promise::ok(())
    }

    fn transaction_removed_from_mempool(
        &mut self,
        _: TransactionRemovedFromMempoolParams,
        _: TransactionRemovedFromMempoolResults,
    ) -> ::capnp::capability::Promise<(), ::capnp::Error> {
        // BDK's transaction graph is monotone so we can't remove the tx here (see https://docs.rs/bdk_chain/latest/bdk_chain/tx_graph/index.html).
        ::capnp::capability::Promise::ok(())
    }

    fn block_connected(
        &mut self,
        params: BlockConnectedParams,
        _: BlockConnectedResults,
    ) -> ::capnp::capability::Promise<(), ::capnp::Error> {
        // Assume no background chainstate for the purpose of this PoC.
        let info = pry!(pry!(params.get()).get_block());
        let height = info.get_height();
        let block = bitcoin::Block::consensus_decode(&mut pry!(info.get_data()))
            .expect("Core must provide valid transactions.");
        println!("New connected block {}.", block.block_hash());
        if let Err(e) = self.lock().unwrap().apply_block(&block, height) {
            eprintln!(
                "Error when applying connected block {}: '{}'",
                block.block_hash(),
                e
            );
        }
        ::capnp::capability::Promise::ok(())
    }

    fn block_disconnected(
        &mut self,
        params: BlockDisconnectedParams,
        _: BlockDisconnectedResults,
    ) -> ::capnp::capability::Promise<(), ::capnp::Error> {
        // Here again, BDK's tx graph is monotone so we don't actually have to remove transactions.
        let info = pry!(pry!(params.get()).get_block());
        let height: u32 = info.get_height().try_into().expect("Can't be negative.");
        let hash = bitcoin::BlockHash::from_slice(pry!(info.get_hash()))
            .expect("Core must provide valid block hashes");
        self.lock()
            .unwrap()
            .disconnect(BlockId { height, hash })
            .expect("Core will never disconnect the genesis block.");
        println!("Disconnected block {}", hash);
        ::capnp::capability::Promise::ok(())
    }

    fn updated_block_tip(
        &mut self,
        _: UpdatedBlockTipParams,
        _: UpdatedBlockTipResults,
    ) -> ::capnp::capability::Promise<(), ::capnp::Error> {
        println!("Block tip updated, but i don't know to what!");
        ::capnp::capability::Promise::ok(())
    }

    fn chain_state_flushed(
        &mut self,
        _: ChainStateFlushedParams,
        _: ChainStateFlushedResults,
    ) -> ::capnp::capability::Promise<(), ::capnp::Error> {
        println!("Chainstate flushed.");
        ::capnp::capability::Promise::ok(())
    }
}

// BDK wallet is up and synced with Core. Inform users and register for notifs.
async fn wallet_startup_complete(rpc: &RpcInterface, wallet: BdkWallet) -> Arc<Mutex<BdkWallet>> {
    println!("BDK Core is synced with bitcoin-node.");
    rpc.show_progress("BDK Core startup", 100, true).await;

    let wallet = Arc::new(Mutex::new(wallet));
    rpc.register_notifications(wallet.clone()).await;
    wallet
}

// If a reorg happened while we were not listening to notifications we need to process it
// "manually". Note that BDK does not maintain a state for the tip, only a monotone graph
// of transaction from which it resolves confirmed one against a linked list of headers
// and a set of anchors at query time. So here "processing the reorg" barely means we need
// to disconnect the old tip from the headers linked list (the "local chain") and process
// the blocks from the new chain to make sure we didn't miss any transaction.
async fn wallet_handle_startup_reorg(
    rpc: &RpcInterface,
    mut wallet: BdkWallet,
    node_tip: &BlockId,
    wallet_tip: &BlockId,
) -> Arc<Mutex<BdkWallet>> {
    // Try to find the common ancestor between the node and the wallet, disconnect and
    // re-process the chain from there. If the common ancestor couldn't be found, use the
    // genesis block.
    // FIXME: we should not disconnect the common ancestor but start from the next one. Doesn't
    // matter for this PoC.
    let common_ancestor = rpc
        .common_ancestor(&node_tip.hash, &wallet_tip.hash)
        .await
        .unwrap_or_else(|| BlockId {
            height: 0,
            hash: wallet.genesis_hash(),
        });

    println!("Disconnecting the chain from {:?}", common_ancestor);
    wallet.disconnect(common_ancestor).unwrap();

    // FIXME: of course the tip height might have changed in the meanwhile. Doesn't matter
    // for this PoC.
    println!("Now processing blocks all the way to the tip.");
    let start_height: i32 = common_ancestor.height.try_into().expect("Never negative.");
    for h in start_height..=node_tip.height.try_into().expect("Never negative.") {
        let block = rpc.get_block(&node_tip.hash, h).await;
        wallet.apply_block(&block, h).unwrap();
    }

    wallet_startup_complete(rpc, wallet).await
}

// Start the BDK wallet and sync its state with Core's.
async fn wallet_startup(
    rpc: &RpcInterface,
) -> Result<Arc<Mutex<BdkWallet>>, Box<dyn std::error::Error>> {
    let mut wallet = BdkWallet::new()?;

    rpc.show_progress("BDK Core startup", 1, false).await;

    let node_tip = rpc.get_tip().await;
    let wallet_tip = wallet.tip();
    if wallet_tip == node_tip {
        return Ok(wallet_startup_complete(rpc, wallet).await);
    }

    if wallet_tip.height >= node_tip.height {
        println!("The tip on bitcoin-node was reorged or moved backward.");
        return Ok(wallet_handle_startup_reorg(rpc, wallet, &node_tip, &wallet_tip).await);
    }

    println!(
        "Height on bitcoin-node moved forward. Making sure wallet tip is still in best chain."
    );
    if !rpc.is_in_best_chain(&node_tip.hash, &wallet_tip.hash).await {
        println!("Wallet tip is not in best chain anymore. Proceeding to process reorg.");
        return Ok(wallet_handle_startup_reorg(rpc, wallet, &node_tip, &wallet_tip).await);
    }

    println!("All good. Now making sure it has all the blocks for us to sync.");
    let start_height: i32 = (wallet_tip.height + 1).try_into().expect("Must fit");
    if !rpc.has_blocks(&node_tip.hash, start_height).await {
        return Err("bitcoin-node is missing blocks to sync the BDK wallet.".into());
    }

    println!("It does. Now proceeding to sync the BDK wallet.");
    for h in start_height..=node_tip.height.try_into().expect("Never negative.") {
        let block = rpc.get_block(&node_tip.hash, h).await;
        if h % 1000 == 0 {
            println!("Syncing block {}", h);
        }
        wallet.apply_block(&block, h)?;
    }

    println!("Done syncing missing blocks.");
    return Ok(wallet_startup_complete(rpc, wallet).await);
}

async fn rpc_main(stream: tokio::net::UnixStream) -> Result<(), Box<dyn std::error::Error>> {
    let rpc = RpcInterface::new(stream).await?;
    let wallet = wallet_startup(&rpc).await?;

    wallet.lock().unwrap().print_info()?;

    println!(
        "\nWaiting {} seconds before disconnecting.",
        SLEEP_BEFORE_DISCONNECT_SECS
    );
    tokio::time::sleep(Duration::from_secs(SLEEP_BEFORE_DISCONNECT_SECS)).await;
    println!("Disconnecting.");
    rpc.disconnect().await?;

    wallet.lock().unwrap().print_info()?;

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: {} /path/to/bitcoin-node/unix/socket", args[0]);
        return Ok(());
    }

    let socket_path = args.last().expect("Just checked.");
    let stream = tokio::net::UnixStream::connect(&socket_path).await?;

    tokio::task::LocalSet::new()
        .run_until(rpc_main(stream))
        .await
}
