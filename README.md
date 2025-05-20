# BDK Core PoC

A PoC of a Rust Bitcoin Core wallet using [BDK](https://github.com/bitcoindevkit) and the `Chain` IPC
interface introduced in Bitcoin Core PR [#29409](https://github.com/bitcoin/bitcoin/pull/29409).

## Usage

This program takes a single parameter, a path to a Unix domain socket to connect to a running
`bitcoin-node` process over IPC. The program will create a new BDK wallet tracking a descriptor
defined as a constant in [`src/main.rs`](src/main.rs). Feel free to change the constant to a
descriptor of your liking. At startup the program will sync the wallet to the height of the running
`bitcoin-node` process and will listen for (dis)connected blocks and transactions entering the
mempool. The state of the wallet is printed at startup, teardown, and whenever it's updated (for
instance if a connected block contains a transaction involving the wallet). By default the program
will stop after 1 minute. This is also defined as a constant at the top of
[`src/main.rs`](src/main.rs) which you should feel free to update. The BDK wallet is persisted
across runs as a `bdk_core_store.dat` file in the current working directory.

For instance:
```
cargo build && ./target/debug/core_bdk_wallet /home/darosior/.bitcoin/regtest/node.sock
```

This will run on regtest by default. You can change the network simply by updating the corresponding
constant at the top of [`src/main.rs`](src/main.rs).

Here is a quick guide to experiment with the program on Regtest.

### Regtest showcase

This assumes you have two folders `bitcoin` (the Bitcoin Core source repository) and
`core_bdk_wallet` (this repo) in your current working directory. You checked out Bitcoin Core PR [#29409](https://github.com/bitcoin/bitcoin/pull/29409)
in the `bitcoin` repo.

#### 1. Compile Bitcoin Core PR #29409

```
cd bitcoin
cmake -B multiprocbuild/ -DWITH_MULTIPROCESS=ON
cmake --build multiprocbuild/ -j20
```

The rest of this guide will assume this is in a folder `bitcoin`.

#### 2. Run two regtest nodes, one `bitcoind` and one `bitcoin-node`

In order to have a readily available funded wallet we'll use a `bitcoind` with a regular Bitcoin
Core wallet on it, connected to the `bitcoin-node` node.

Start the `bitcoind` node, create a wallet on it (i'll call it `alice`), and fund the wallet by
mining some blocks:
```
mkdir regular_bitcoind_wallet
./multiprocbuild/src/bitcoind -regtest -datadir=$PWD/regular_bitcoind_wallet -daemon
./multiprocbuild/src/bitcoin-cli -regtest -datadir=$PWD/regular_bitcoind_wallet createwallet alice
./multiprocbuild/src/bitcoin-cli -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice generatetoaddress 110 $(./multiprocbuild/src/bitcoin-cli -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice getnewaddress)
```

Now start the `bitcoin-node` node in a different datadir, connected to the first node, with no
JSONRPC server:
```
mkdir datadir_bdk_wallet
./multiprocbuild/src/bitcoin-node -regtest -datadir=$PWD/datadir_bdk_wallet -server=0 -port=19444 -connect=127.0.0.1:18444 -ipcbind=unix -debug=ipc
```

*(Mind the necessary `-ipcbind=unix` to create the interface and optional `-debug=ipc` to observe IPC
messages.)*

#### 3. Build the Rust wallet, connect it and test a few scenarii

From the parent directory.

```
cd core_bdk_wallet
cargo build
./target/debug/core_bdk_wallet ../bitcoin/datadir_bdk_wallet/regtest/node.sock
```

Here is the output:
```
Height on bitcoin-node moved forward. Making sure wallet tip is still in best chain.
All good. Now making sure it has all the blocks for us to sync.
It does. Now proceeding to sync the BDK wallet.
Done syncing missing blocks.
BDK Core is synced with bitcoin-node.
Wallet info:
      Next unused address: bcrt1p6cr3s4qctntvjecaujv8ewqe3xlcz4kz2a6hp6v3t5ja40hzwzfstmrw9d.
      Next unrevealed address: bcrt1pvq3qt00md83428aupply35pt5vvf6af62k790gm95l4utz405npsxtp566.
      Balance (confirmed + unconfirmed): 0 BTC.
      Utxos:
      Transactions:

Waiting 60 seconds before disconnecting.
Disconnecting.
Wallet info:
      Next unused address: bcrt1p6cr3s4qctntvjecaujv8ewqe3xlcz4kz2a6hp6v3t5ja40hzwzfstmrw9d.
      Next unrevealed address: bcrt1p3h6lks0rg95yjv07dzmhv0jzqcx23gk47urq8lwmrkeqhmqte9ns6s53s8.
      Balance (confirmed + unconfirmed): 0 BTC.
      Utxos:
      Transactions:
```

##### Receive unconfirmed funds on the BDK Core wallet

When starting up the BDK Core wallet will give you an unused address. Use it to send fund to it from
the regular Bitcoin Core wallet.

**Mind to restart your BDK Core wallet if necessary**, as it disconnects after 60 seconds from
startup by default.

From the `bitcoin` folder (replace the address with yours):
```
./multiprocbuild/src/bitcoin-cli -named -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice sendtoaddress bcrt1p6cr3s4qctntvjecaujv8ewqe3xlcz4kz2a6hp6v3t5ja40hzwzfstmrw9d amount=0.4242 fee_rate=1
```

*(Note it takes a few seconds for the first node to relay the transaction to the other one. That's
why your BDK Core wallet only prints the reception notification after a couple seconds.)*

Output:
```
BDK Core is synced with bitcoin-node.
Wallet info:
      Next unused address: bcrt1p6cr3s4qctntvjecaujv8ewqe3xlcz4kz2a6hp6v3t5ja40hzwzfstmrw9d.
      Next unrevealed address: bcrt1p0d4el9qd63w3xlyqv9wqcjuqaypk8pasef333qalq4mzes59pw4q60nz3j.
      Balance (confirmed + unconfirmed): 0 BTC.
      Utxos:
      Transactions:

Waiting 60 seconds before disconnecting.
New mempool transaction 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50.
Graph change set not empty. Here is the new state of the wallet.
Wallet info:
      Next unused address: bcrt1pvq3qt00md83428aupply35pt5vvf6af62k790gm95l4utz405npsxtp566.
      Next unrevealed address: bcrt1p7cmkfm0s8prq93vur3r8xvtclq53rfyuadypxasg9xfa774k3yyscyzkt2.
      Balance (confirmed + unconfirmed): 0.42420000 BTC.
      Utxos: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50:0 (0.42420000 BTC),
      Transactions: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50 (chain pos: Some(Unconfirmed(0))),
Disconnecting.
Wallet info:
      Next unused address: bcrt1pvq3qt00md83428aupply35pt5vvf6af62k790gm95l4utz405npsxtp566.
      Next unrevealed address: bcrt1peast0x5kymt6ta7ssrslpydn423g6hj9se9ckmjvuem5ks434qaskpxq05.
      Balance (confirmed + unconfirmed): 0.42420000 BTC.
      Utxos: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50:0 (0.42420000 BTC),
      Transactions: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50 (chain pos: Some(Unconfirmed(0))),
```

##### Receive confirmed funds on the BDK Core wallet

Now lets make another transaction and mine a block which will confirm both this new transaction and
the one we did just before.

**Again, make sure your BDK Core wallet is still up.**

```
./multiprocbuild/src/bitcoin-cli -named -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice sendtoaddress bcrt1pvq3qt00md83428aupply35pt5vvf6af62k790gm95l4utz405npsxtp566 amount=0.2121 fee_rate=1
./multiprocbuild/src/bitcoin-cli -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice generatetoaddress 1 $(./multiprocbuild/src/bitcoin-cli -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice getnewaddress)
```

*(Note i used the following unused address for the second transaction.)*

Output:
```
BDK Core is synced with bitcoin-node.
Wallet info:
      Next unused address: bcrt1pvq3qt00md83428aupply35pt5vvf6af62k790gm95l4utz405npsxtp566.
      Next unrevealed address: bcrt1par8p4jjqhqf6x6r6zpashsmvrvw8wrqxh9u6zagjwehxtwcp6cgqylj2qu.
      Balance (confirmed + unconfirmed): 0 BTC.
      Utxos:
      Transactions: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50 (chain pos: None),

Waiting 60 seconds before disconnecting.
New mempool transaction 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6.
Graph change set not empty. Here is the new state of the wallet.
Wallet info:
      Next unused address: bcrt1p3h6lks0rg95yjv07dzmhv0jzqcx23gk47urq8lwmrkeqhmqte9ns6s53s8.
      Next unrevealed address: bcrt1p6k4nynwt9j0rejqc5kktdtsx82jx0t54pdm8vj04akfsh8le4ymsjs8kvm.
      Balance (confirmed + unconfirmed): 0.21210000 BTC.
      Utxos: 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6:1 (0.21210000 BTC),
      Transactions: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50 (chain pos: None), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6 (chain pos: Some(Unconfirmed(0))),
New connected block 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702.
Graph change set not empty. Here is the new state of the wallet.
Wallet info:
      Next unused address: bcrt1p3h6lks0rg95yjv07dzmhv0jzqcx23gk47urq8lwmrkeqhmqte9ns6s53s8.
      Next unrevealed address: bcrt1p9whnp24ra6y7t2h3cqdstajjpwm4n40dd7c6349v0k48eazvujvsmlp87g.
      Balance (confirmed + unconfirmed): 0.63630000 BTC.
      Utxos: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50:0 (0.42420000 BTC), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6:1 (0.21210000 BTC),
      Transactions: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702 }, confirmation_time: 1733513496 }))), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702 }, confirmation_time: 1733513496 }))),
Block tip updated, but i don't know to what!
Disconnecting.
Wallet info:
      Next unused address: bcrt1p3h6lks0rg95yjv07dzmhv0jzqcx23gk47urq8lwmrkeqhmqte9ns6s53s8.
      Next unrevealed address: bcrt1p2et26ujjf4yqlcrcc0ezjde30phlv2wpp68jqay84qmq56nkfezsq0l660.
      Balance (confirmed + unconfirmed): 0.63630000 BTC.
      Utxos: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50:0 (0.42420000 BTC), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6:1 (0.21210000 BTC),
      Transactions: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702 }, confirmation_time: 1733513496 }))), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702 }, confirmation_time: 1733513496 }))),
```

##### Receive funds on the BDK Core wallet while it is off

Wait for your BDK Core wallet to disconnect and send it another confirmed transaction.

```
./multiprocbuild/src/bitcoin-cli -named -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice sendtoaddress bcrt1p2et26ujjf4yqlcrcc0ezjde30phlv2wpp68jqay84qmq56nkfezsq0l660 amount=12 fee_rate=1
./multiprocbuild/src/bitcoin-cli -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice generatetoaddress 1 $(./multiprocbuild/src/bitcoin-cli -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice getnewaddress
```

Now start it again, it will sync against Core and find out the transaction in the block:
```
Height on bitcoin-node moved forward. Making sure wallet tip is still in best chain.
All good. Now making sure it has all the blocks for us to sync.
It does. Now proceeding to sync the BDK wallet.
Graph change set not empty. Here is the new state of the wallet.
Wallet info:
      Next unused address: bcrt1p3h6lks0rg95yjv07dzmhv0jzqcx23gk47urq8lwmrkeqhmqte9ns6s53s8.
      Next unrevealed address: bcrt1pzq4cs5a4naz4wmn98hdwug85mmer06yqhalkld2zrh43aqxuzews5peykx.
      Balance (confirmed + unconfirmed): 12.63630000 BTC.
      Utxos: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50:0 (0.42420000 BTC), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6:1 (0.21210000 BTC), eb097f0628b63b9bdd8333aeb092f69ed5f38b7715acadeb77d87771b25a7c00:1 (12 BTC), 
      Transactions: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702 }, confirmation_time: 1733513496 }))), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702 }, confirmation_time: 1733513496 }))), eb097f0628b63b9bdd8333aeb092f69ed5f38b7715acadeb77d87771b25a7c00 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 112, hash: 6b68ef5d2778893109216da3b0d6b3ec6744b8137d6acf69a308c6daaa75e71f }, confirmation_time: 1733513716 }))), 
Done syncing missing blocks.
BDK Core is synced with bitcoin-node.
Wallet info:
      Next unused address: bcrt1p3h6lks0rg95yjv07dzmhv0jzqcx23gk47urq8lwmrkeqhmqte9ns6s53s8.
      Next unrevealed address: bcrt1p8lk730mlsq34zerrycex9n4h4vhyqqn2qy4tku78k0krshxkgz5qv0hpz0.
      Balance (confirmed + unconfirmed): 12.63630000 BTC.
      Utxos: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50:0 (0.42420000 BTC), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6:1 (0.21210000 BTC), eb097f0628b63b9bdd8333aeb092f69ed5f38b7715acadeb77d87771b25a7c00:1 (12 BTC), 
      Transactions: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702 }, confirmation_time: 1733513496 }))), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702 }, confirmation_time: 1733513496 }))), eb097f0628b63b9bdd8333aeb092f69ed5f38b7715acadeb77d87771b25a7c00 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 112, hash: 6b68ef5d2778893109216da3b0d6b3ec6744b8137d6acf69a308c6daaa75e71f }, confirmation_time: 1733513716 }))), 

Waiting 60 seconds before disconnecting.
```

##### Handle a reorg when up

**Make sure your BDK Core wallet is still up, or start it again.**

Create a fake reorg from the `bitcoind` node to propagate to the `bitcoin-node` node.
```
./multiprocbuild/src/bitcoin-cli -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice invalidateblock $(./multiprocbuild/src/bitcoin-cli -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice getblockhash 111)
./multiprocbuild/src/bitcoin-cli -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice generatetoaddress 3 $(./multiprocbuild/src/bitcoin-cli -regtest -datadir=$PWD/regular_bitcoind_wallet -rpcwallet=alice getnewaddress)
```

Output:
```
BDK Core is synced with bitcoin-node.
Wallet info:
      Next unused address: bcrt1p3h6lks0rg95yjv07dzmhv0jzqcx23gk47urq8lwmrkeqhmqte9ns6s53s8.
      Next unrevealed address: bcrt1p4jzzaznwpkkf4ltf55sxgz4qdaa508k0duq0lvpsk2vy9e6l06nqnwksv8.
      Balance (confirmed + unconfirmed): 12.63630000 BTC.
      Utxos: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50:0 (0.42420000 BTC), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6:1 (0.21210000 BTC), eb097f0628b63b9bdd8333aeb092f69ed5f38b7715acadeb77d87771b25a7c00:1 (12 BTC),
      Transactions: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702 }, confirmation_time: 1733513496 }))), eb097f0628b63b9bdd8333aeb092f69ed5f38b7715acadeb77d87771b25a7c00 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 112, hash: 6b68ef5d2778893109216da3b0d6b3ec6744b8137d6acf69a308c6daaa75e71f }, confirmation_time: 1733513716 }))), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702 }, confirmation_time: 1733513496 }))),

Waiting 60 seconds before disconnecting.
Disconnected block 6b68ef5d2778893109216da3b0d6b3ec6744b8137d6acf69a308c6daaa75e71f
Disconnected block 4104aedca24847bd3bca61dfc1150607a50fe76ff88079e5733eccdd3b589702
New mempool transaction eb097f0628b63b9bdd8333aeb092f69ed5f38b7715acadeb77d87771b25a7c00.
New connected block 540ae0da4d8416adea2b2a3c368956c3cc8b1fbc1682e35c5c0b87bda69bcc3a.
Graph change set not empty. Here is the new state of the wallet.
Wallet info:
      Next unused address: bcrt1p3h6lks0rg95yjv07dzmhv0jzqcx23gk47urq8lwmrkeqhmqte9ns6s53s8.
      Next unrevealed address: bcrt1p7a67ldf3pzc3fmu625kt0nnk0kzy5357ef7f87p49406s73ljvnspfuyvc.
      Balance (confirmed + unconfirmed): 12.63630000 BTC.
      Utxos: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50:0 (0.42420000 BTC), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6:1 (0.21210000 BTC), eb097f0628b63b9bdd8333aeb092f69ed5f38b7715acadeb77d87771b25a7c00:1 (12 BTC),
      Transactions: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 540ae0da4d8416adea2b2a3c368956c3cc8b1fbc1682e35c5c0b87bda69bcc3a }, confirmation_time: 1733514139 }))), eb097f0628b63b9bdd8333aeb092f69ed5f38b7715acadeb77d87771b25a7c00 (chain pos: Some(Unconfirmed(0))), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 540ae0da4d8416adea2b2a3c368956c3cc8b1fbc1682e35c5c0b87bda69bcc3a }, confirmation_time: 1733514139 }))),
New connected block 1d3188008757b4e6fdf709e021fd4bd3018566aaac3956445a62595d9f6d4709.
New connected block 44441dc19b1ef15becd7907b424f64e1bfddeed839a6e00f107c1023c738f748.
Block tip updated, but i don't know to what!
Disconnecting.
Wallet info:
      Next unused address: bcrt1p3h6lks0rg95yjv07dzmhv0jzqcx23gk47urq8lwmrkeqhmqte9ns6s53s8.
      Next unrevealed address: bcrt1p6928jldavh4qmepzhsv0vagsxtvsv7acplgcmzfgevv6w388y0psrdhfw4.
      Balance (confirmed + unconfirmed): 12.63630000 BTC.
      Utxos: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50:0 (0.42420000 BTC), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6:1 (0.21210000 BTC), eb097f0628b63b9bdd8333aeb092f69ed5f38b7715acadeb77d87771b25a7c00:1 (12 BTC),
      Transactions: 4403bb5d4e3ac0fba62c566681248badc006fa771c891e9b05b6c51d8dba8d50 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 540ae0da4d8416adea2b2a3c368956c3cc8b1fbc1682e35c5c0b87bda69bcc3a }, confirmation_time: 1733514139 }))), eb097f0628b63b9bdd8333aeb092f69ed5f38b7715acadeb77d87771b25a7c00 (chain pos: Some(Unconfirmed(0))), 5d6135eb0b73b5642cc9898eda489fc6794950ad5cb15e8f82562b1fbdede5f6 (chain pos: Some(Confirmed(ConfirmationBlockTime { block_id: BlockId { height: 111, hash: 540ae0da4d8416adea2b2a3c368956c3cc8b1fbc1682e35c5c0b87bda69bcc3a }, confirmation_time: 1733514139 }))),
```

##### Your favourite scenario

You can also do all this when the wallet is down. It's also possible to test more of the `Chain` IPC
interface by implementing more methods and finding ways to use them along with the BDK wallet. Some
candidates:
- Use `findCoins` with wallet coins to scan the mempool at startup
- Create a spending transaction and broadcast it using the `broadcastTransaction` method
- Query ancestry/package information about the wallet's unconfirmed transactions
- Query fee estimates

## Generating Rust IPC interface from Capnp definition

To generate the Rust source files i used the [`capnp`](https://capnproto.org/capnp-tool.html) tool.
I copied the Capnp files from Bitcoin Core along with its vendored libmultiprocess dependency (`mp`),
and from the Capnp "standard library" (`/capnp`), in the `schema` folder at the root of this repository.
I applied only minimal changes to be able to compile them on their own (no system dep) and result in
Rust source files at the root.

```
mkdir schema
cp ../bitcoin/src/ipc/capnp/*.capnp schema/
cp ../bitcoin/src/ipc/libmultiprocess/include/mp/*.capnp schema/
cp /usr/include/capnp/c++.capnp schema/
sed -i 's/"\/mp\//"/g' schema/*.capnp
sed -i 's/"\/capnp\//"/g' schema/*.capnp
sed -i 's/"..\/capnp\//"/g' schema/*.capnp
capnp compile --no-standard-import --import-path=$PWD/schema --output=rust:src/ --src-prefix=schema schema/c++.capnp schema/proxy.capnp schema/init.capnp schema/chain.capnp schema/common.capnp schema/echo.capnp schema/handler.capnp schema/mining.capnp
```
