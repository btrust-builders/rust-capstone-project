#![allow(unused)]
use bitcoin::hex::DisplayHex;
use bitcoincore_rpc::bitcoin::Amount;
use bitcoincore_rpc::bitcoin::Txid;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use serde::Deserialize;
use serde_json::json;
use std::fmt::format;
use std::fs::File;
use std::io::Write;
use std::path::Path;

// Node access params
const RPC_URL: &str = "http://127.0.0.1:18443"; // Default regtest RPC port
const RPC_USER: &str = "alice";
const RPC_PASS: &str = "password";

// You can use calls not provided in RPC lib API using the generic `call` function.
// An example of using the `send` RPC call, which doesn't have exposed API.
// You can also use serde_json `Deserialize` derivation to capture the returned json result.
fn send(rpc: &Client, addr: &str) -> bitcoincore_rpc::Result<String> {
    let args = [
        json!([{addr : 100 }]), // recipient address
        json!(null),            // conf target
        json!(null),            // estimate mode
        json!(null),            // fee rate in sats/vb
        json!(null),            // Empty option object
    ];

    #[derive(Deserialize)]
    struct SendResult {
        complete: bool,
        txid: String,
    }
    let send_result = rpc.call::<SendResult>("send", &args)?;
    assert!(send_result.complete);
    Ok(send_result.txid)
}

static EMPTY_ADDRS: [bitcoincore_rpc::bitcoin::Address<
    bitcoincore_rpc::bitcoin::address::NetworkUnchecked,
>; 0] = [];
fn main() -> bitcoincore_rpc::Result<()> {
    // Connect to Bitcoin Core RPC
    let rpc = Client::new(
        RPC_URL,
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Get blockchain info
    let blockchain_info = rpc.get_blockchain_info()?;
    println!("Blockchain Info: {:?}", blockchain_info);

    // Create/Load the wallets, named 'Miner' and 'Trader'. Have logic to optionally create/load them if they do not exist or not loaded already.
    for wallet_name in ["Miner", "Trader"] {
        let res = rpc.create_wallet(wallet_name, None, None, None, None);
        match res {
            Ok(_) => println!("Wallet '{wallet_name}' created successfully"),
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("already exists") {
                    println!("Sorry this wallet '{wallet_name}' already exists");
                } else {
                    return Err(e);
                }
            }
        }
    }

    // Separate RPC Client for each wallet
    let miner_wallet = Client::new(
        &format!("{}/wallet/{}", RPC_URL, "Miner"),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    let trader_wallet = Client::new(
        &format!("{}/wallet/{}", RPC_URL, "Trader"),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Generate spendable balances in the Miner wallet. How many blocks needs to be mined?
    // Mining address for mining reward
    let mining_address = miner_wallet
        .get_new_address(Some("Mining Reward"), None)?
        .assume_checked();

    let mut balance = miner_wallet.get_balance(None, None)?.to_btc();
    let mut blocks_mined: i32 = 0;
    while balance <= 0.0 {
        miner_wallet.generate_to_address(1, &mining_address)?;
        blocks_mined += 1;
        balance = miner_wallet.get_balance(None, None)?.to_btc();
    }
    println!("Blocks mined: {}", blocks_mined);

    // Bitcoin block rewards (coinbase transactions) require 100 confirmations to mature and become spendable.
    // This means you need to mine at least 101 blocks (100 confirmations + 1 initial block) to get a positive spendable balance.
    // This is a security feature to prevent issues if the blockchain reorganizes.

    println!("Miner wallet balance: {} BTC", balance);
    // Load Trader wallet and generate a new address
    // The receiving address for trader
    let trader_address = trader_wallet
        .get_new_address(Some("Received"), None)?
        .assume_checked();
    println!("Receiving address for trader: {}", trader_address);

    // Send 20 BTC from Miner to Trader
    let txid = miner_wallet.send_to_address(
        &trader_address,
        Amount::from_btc(20.0)?,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;
    println!("Funds of 20BTC sent from miner to trader with ID: {}", txid);

    // Check transaction in mempool
    let mempool_entry = miner_wallet.get_mempool_entry(&txid);
    println!("The Mempool entry: {mempool_entry:#?} for trans ID: {txid}");

    // Mine 1 block to confirm the transaction
    miner_wallet.generate_to_address(1, &mining_address)?;

    // use bitcoincore_rpc::bitcoin::Txid;
    // use std::path::Path;

    // Extract all required transaction details
    let tx_info = miner_wallet.get_transaction(&txid, None)?;
    let block_hash = tx_info
        .info
        .blockhash
        .expect("Transaction ought to be confirmed in block");
    let block = miner_wallet.get_block_info(&block_hash)?;
    let block_height = block.height;

    let raw_tx = miner_wallet.get_raw_transaction(&txid, Some(&block_hash))?;
    let decoded_tx = miner_wallet.decode_raw_transaction(&raw_tx, None)?;
    let input = &decoded_tx.vin[0];
    let prev_txid = input.txid.expect("Input should have txid");
    let prev_vout = input.vout.expect("Input should have vout") as usize;
    let prev_tx = miner_wallet.get_raw_transaction(&prev_txid, None)?;
    let prev_decoded = miner_wallet.decode_raw_transaction(&prev_tx, None)?;
    let prev_output = &prev_decoded.vout[prev_vout];
    let input_addresses = &prev_output.script_pub_key.addresses;
    let miner_input_address: String = input_addresses
        .first()
        .map(|a| format!("{}", a.clone().assume_checked()))
        .unwrap_or_default();
    let miner_input_amount = prev_output.value.to_btc();
    // Trader's output and miner's change
    let mut trader_output_address: String = String::new();
    let mut trader_output_amount: f64 = 0.0;
    let mut miner_change_address = String::new();
    let mut miner_change_amount: f64 = 0.0;
    for vout in &decoded_tx.vout {
        if let Some(addr) = &vout.script_pub_key.address {
            let addr_str = addr.clone().assume_checked().to_string();
            if addr_str == trader_address.to_string() {
                trader_output_address = addr_str.clone();
                trader_output_amount = vout.value.to_btc();
            } else {
                let info = miner_wallet.get_address_info(&addr.clone().assume_checked());
                if let Ok(address_info) = info {
                    if address_info.is_mine.unwrap_or(false) {
                        miner_change_address = addr_str.clone();
                        miner_change_amount = vout.value.to_btc();
                    }
                }
            }
        }
    }

    let tx_fee = miner_input_amount - (trader_output_amount + miner_change_amount);

    // Write the data to ../out.txt in the specified format given in readme.md
    let out_path = Path::new("../out.txt");
    let mut out_file = File::create(out_path)?;
    writeln!(out_file, "{txid}")?;
    writeln!(out_file, "{miner_input_address}")?;
    writeln!(
        out_file,
        "{}",
        if miner_input_amount.fract() == 0.0 {
            format!("{:.0}", miner_input_amount)
        } else {
            format!("{:.8}", miner_input_amount)
        }
    )?;
    writeln!(out_file, "{trader_output_address}")?;
    writeln!(
        out_file,
        "{}",
        if trader_output_amount.fract() == 0.0 {
            format!("{:.0}", trader_output_amount)
        } else {
            format!("{:.8}", trader_output_amount)
        }
    )?;
    writeln!(out_file, "{miner_change_address}")?;
    writeln!(out_file, "{:.8}", miner_change_amount)?;
    writeln!(out_file, "{:.8}", tx_fee.abs())?;
    writeln!(out_file, "{block_height}")?;
    writeln!(out_file, "{block_hash}")?;

    Ok(())
}


