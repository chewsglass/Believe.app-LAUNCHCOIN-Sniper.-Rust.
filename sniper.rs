use async_println::async_log;
use async_println::pack_log_async;
use bs58;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use reqwest;
use serde_json::{json, Value};
use solana_client::rpc_client::RpcClient;
use solana_program::program_error::ProgramError;
use solana_sdk::{
    commitment_config::{CommitmentConfig, CommitmentLevel},
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signature::Signature,
    signer::{keypair::Keypair, Signer},
    system_instruction,
    transaction::Transaction,
    transaction::VersionedTransaction,
};
use spl_associated_token_account::instruction::create_associated_token_account_idempotent;
use spl_token::instruction::sync_native;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;

// user config v
const USE_JITO: bool = true;
const JITO_URL: &str = "https://amsterdam.mainnet.block-engine.jito.wtf/api/v1/bundles";
const JITO_TIP_ACCOUNT: &str = "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY";
const TIP_AMOUNT: u64 = 100000; // 0.0001 SOL in lamports
const SOL_AMOUNT_IN: f64 = 0.0001;
const MIN_TOKENS_OUT: u64 = 1111111;
const COMPUTE_UNIT_PRICE: u64 = 1000000;
// user config ^


// dont touch v
const MAX_RETRIES: u32 = 300;
const PROGRAM_ID: &str = "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN";
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const METAPLEX_KEY: &str = "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s";
const TARGET_ACCOUNTS: [&str; 2] = [
    "5qWya6UjwWnGVhdSBL3hyZ7B45jbk6Byt1hwd7ohEGXE",
    "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN",
];
// dont touch ^
// BUY_AFTER supports multiple condition types:
// - "transfer"                                    -> Wait for transfer instruction
// - "transferChecked"                             -> Wait for transferChecked instruction
// - "swap"                                        -> Wait for swap instruction
// - "pubkey:SPECIFIC_PUBKEY_HERE"                 -> Wait for specific pubkey in transaction
// - "pubkey_any:WALLET1,WALLET2,WALLET3"         -> Wait for ANY of these pubkeys (OR logic)
// - "program:PROGRAM_ID_HERE"                     -> Wait for specific program involvement
// - "amount_over:1000000"                         -> Wait for SOL amount over 0.001 SOL (SOL only)
// - "amount_under:5000000"                        -> Wait for SOL amount under 0.005 SOL (SOL only)
// - "sol_over:0.1"                                -> Wait for SOL amount over 0.1 SOL (SOL transfers only)
// - "sol_under:1.0"                               -> Wait for SOL amount under 1.0 SOL (SOL transfers only)
// - "token_amount_over:1000"                      -> Wait for token amount over 1000 tokens (token transfers only)
//
// Examples:
// - &["transfer"]                                 -> Buy after any transfer
// - &["transfer", "transferChecked"]              -> Buy after BOTH transfer AND transferChecked
// - &["pubkey:So11111111111111111111111111111111111111112"] -> Buy when WSOL is involved
// - &["program:11111111111111111111111111111111"] -> Buy when System Program is used
// - &["transfer", "sol_over:0.001"]               -> Buy when SOL transfer over 0.001 SOL (ignores token transfers)
// - &["swap", "sol_over:0.5"]                     -> Buy when swap involves over 0.5 SOL
// - &["transfer", "pubkey_any:WALLET1,WALLET2"]   -> Buy when transfer AND (WALLET1 OR WALLET2)
// - &["transfer", "pubkey:SOME_WHALE_WALLET"]     -> Buy when SOL transfer happens AND whale wallet involved
const BUY_AFTER: &[&str] = &["transfer"];

#[derive(Debug)]
enum StoredTransaction {
    Regular(Transaction),
    Jito(VersionedTransaction),
}

#[derive(Debug)]
struct ProcessError(String);

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ProcessError {}

impl From<&str> for ProcessError {
    fn from(e: &str) -> Self {
        ProcessError(e.to_string())
    }
}

impl From<String> for ProcessError {
    fn from(e: String) -> Self {
        ProcessError(e)
    }
}

impl From<solana_client::client_error::ClientError> for ProcessError {
    fn from(e: solana_client::client_error::ClientError) -> Self {
        ProcessError(e.to_string())
    }
}

impl From<solana_sdk::pubkey::ParsePubkeyError> for ProcessError {
    fn from(e: solana_sdk::pubkey::ParsePubkeyError) -> Self {
        ProcessError(e.to_string())
    }
}

impl From<serde_json::Error> for ProcessError {
    fn from(e: serde_json::Error) -> Self {
        ProcessError(e.to_string())
    }
}

impl From<hex::FromHexError> for ProcessError {
    fn from(e: hex::FromHexError) -> Self {
        ProcessError(e.to_string())
    }
}

impl From<spl_token::error::TokenError> for ProcessError {
    fn from(e: spl_token::error::TokenError) -> Self {
        ProcessError(e.to_string())
    }
}

impl From<url::ParseError> for ProcessError {
    fn from(e: url::ParseError) -> Self {
        ProcessError(e.to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for ProcessError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        ProcessError(e.to_string())
    }
}

impl From<ProgramError> for ProcessError {
    fn from(e: ProgramError) -> Self {
        ProcessError(e.to_string())
    }
}

async fn get_owner_ata(mint: &Pubkey, owner: &Pubkey) -> Pubkey {
    let (ata, _) = Pubkey::find_program_address(
        &[owner.as_ref(), spl_token::id().as_ref(), mint.as_ref()],
        &spl_associated_token_account::id(),
    );
    ata
}

async fn build_buy_transaction(
    client: &RpcClient,
    wallet: &solana_sdk::signer::keypair::Keypair,
    keys: &[String],
) -> Result<StoredTransaction, ProcessError> {
    let wsol_mint = Pubkey::from_str(WSOL_MINT)?;
    let base_mint = Pubkey::from_str(&keys[3])?;
    let program_id = Pubkey::from_str(PROGRAM_ID)?;

    let quote_ata = get_owner_ata(&wsol_mint, &wallet.pubkey()).await;
    let base_ata = get_owner_ata(&base_mint, &wallet.pubkey()).await;

    let sol_amount_lamports = (SOL_AMOUNT_IN * 1_000_000_000.0) as u64;

    let discriminator = hex::decode("f8c69e91e17587c8")?;
    let sol_amount_bytes = sol_amount_lamports.to_le_bytes();
    let min_tokens_bytes = MIN_TOKENS_OUT.to_le_bytes();

    let mut instruction_data = Vec::new();
    instruction_data.extend_from_slice(&discriminator);
    instruction_data.extend_from_slice(&sol_amount_bytes);
    instruction_data.extend_from_slice(&min_tokens_bytes);

    let mut instructions = Vec::new();

    instructions.push(ComputeBudgetInstruction::set_compute_unit_price(
        COMPUTE_UNIT_PRICE,
    ));

    instructions.push(create_associated_token_account_idempotent(
        &wallet.pubkey(),
        &wallet.pubkey(),
        &wsol_mint,
        &spl_token::id(),
    ));

    instructions.push(create_associated_token_account_idempotent(
        &wallet.pubkey(),
        &wallet.pubkey(),
        &base_mint,
        &spl_token::id(),
    ));

    instructions.push(system_instruction::transfer(
        &wallet.pubkey(),
        &quote_ata,
        sol_amount_lamports,
    ));

    instructions.push(sync_native(&spl_token::id(), &quote_ata)?);

    let swap_instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(Pubkey::from_str(&keys[1])?, false),
            AccountMeta::new_readonly(Pubkey::from_str(&keys[0])?, false),
            AccountMeta::new(Pubkey::from_str(&keys[5])?, false),
            AccountMeta::new(quote_ata, false),
            AccountMeta::new(base_ata, false),
            AccountMeta::new(Pubkey::from_str(&keys[6])?, false),
            AccountMeta::new(Pubkey::from_str(&keys[7])?, false),
            AccountMeta::new(base_mint, false),
            AccountMeta::new_readonly(Pubkey::from_str(&keys[4])?, false),
            AccountMeta::new(wallet.pubkey(), true),
            AccountMeta::new_readonly(Pubkey::from_str(&keys[11])?, false),
            AccountMeta::new_readonly(Pubkey::from_str(&keys[12])?, false),
            AccountMeta::new_readonly(program_id, false),
            AccountMeta::new_readonly(Pubkey::from_str(&keys[14])?, false),
            AccountMeta::new_readonly(Pubkey::from_str(&keys[15])?, false),
        ],
        data: instruction_data,
    };

    instructions.push(swap_instruction);

    if USE_JITO {
        // Add JITO tip instruction
        let jito_tip = create_jito_tip_instruction(wallet).await?;
        instructions.push(jito_tip);

        // Create versioned transaction
        let versioned_tx = create_versioned_transaction(client, wallet, instructions).await?;
        Ok(StoredTransaction::Jito(versioned_tx))
    } else {
        // Create regular transaction
        let mut transaction = Transaction::new_with_payer(&instructions, Some(&wallet.pubkey()));
        let recent_blockhash = client.get_latest_blockhash()?;
        transaction.sign(&[wallet], recent_blockhash);
        Ok(StoredTransaction::Regular(transaction))
    }
}

async fn submit_transaction_with_retries(
    client: &RpcClient,
    transaction: &Transaction,
) -> Result<Signature, ProcessError> {
    for attempt in 0..MAX_RETRIES {
        match client.send_transaction_with_config(
            transaction,
            solana_client::rpc_config::RpcSendTransactionConfig {
                skip_preflight: true,
                preflight_commitment: Some(CommitmentLevel::Confirmed),
                ..Default::default()
            },
        ) {
            Ok(signature) => {
                return Ok(signature);
            }
            Err(_) => {
                if attempt == MAX_RETRIES - 1 {
                    return Err(ProcessError(format!(
                        "failed after {} attempts",
                        MAX_RETRIES
                    )));
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }

    Err(ProcessError("maximum retries exceeded".to_string()))
}

async fn create_jito_tip_instruction(wallet: &Keypair) -> Result<Instruction, ProcessError> {
    let jito_tip_account = Pubkey::from_str(JITO_TIP_ACCOUNT)?;
    Ok(system_instruction::transfer(
        &wallet.pubkey(),
        &jito_tip_account,
        TIP_AMOUNT,
    ))
}

async fn create_versioned_transaction(
    client: &RpcClient,
    wallet: &Keypair,
    instructions: Vec<Instruction>,
) -> Result<VersionedTransaction, ProcessError> {
    let recent_blockhash = client.get_latest_blockhash()?;

    let message = v0::Message::try_compile(&wallet.pubkey(), &instructions, &[], recent_blockhash)
        .map_err(|e| ProcessError(e.to_string()))?;

    let versioned_message = VersionedMessage::V0(message);
    let versioned_transaction = VersionedTransaction::try_new(versioned_message, &[wallet])
        .map_err(|e| ProcessError(e.to_string()))?;

    Ok(versioned_transaction)
}

async fn send_jito_bundle(transactions: Vec<VersionedTransaction>) -> Result<Value, ProcessError> {
    let client = reqwest::Client::new();

    // Serialize and encode transactions
    let encoded_transactions: Vec<String> = transactions
        .into_iter()
        .map(|tx| {
            let serialized = bincode::serialize(&tx).map_err(|e| ProcessError(e.to_string()))?;
            Ok(bs58::encode(serialized).into_string())
        })
        .collect::<Result<Vec<String>, ProcessError>>()?;

    let data = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendBundle",
        "params": [encoded_transactions]
    });

    let response = client
        .post(JITO_URL)
        .header("Content-Type", "application/json")
        .json(&data)
        .send()
        .await
        .map_err(|e| ProcessError(e.to_string()))?;

    let result: Value = response
        .json()
        .await
        .map_err(|e| ProcessError(e.to_string()))?;

    Ok(result)
}

async fn submit_stored_transaction(
    client: &RpcClient,
    _wallet: &Keypair,
    stored_transaction: &StoredTransaction,
) -> Result<String, ProcessError> {
    match stored_transaction {
        StoredTransaction::Regular(transaction) => {
            let signature = submit_transaction_with_retries(client, transaction).await?;
            Ok(signature.to_string())
        }
        StoredTransaction::Jito(versioned_tx) => {
            let bundle_result = send_jito_bundle(vec![versioned_tx.clone()]).await?;
            if let Some(result) = bundle_result.get("result") {
                Ok(result.to_string())
            } else {
                Ok("jito_bundle_submitted".to_string())
            }
        }
    }
}

fn get_instruction_type(instruction: &Value) -> Option<String> {
    if let Some(parsed) = instruction.get("parsed") {
        if let Some(type_str) = parsed.get("type").and_then(|t| t.as_str()) {
            return Some(type_str.to_lowercase());
        }
    }
    None
}

fn get_instruction_amount(instruction: &Value) -> Option<u64> {
    if let Some(parsed) = instruction.get("parsed") {
        if let Some(info) = parsed.get("info") {
            // Only get SOL amounts for transfer instructions, not token amounts
            if let Some(amount_num) = info.get("lamports").and_then(|a| a.as_u64()) {
                return Some(amount_num);
            }
        }
    }
    None
}

fn get_token_amount(instruction: &Value) -> Option<u64> {
    if let Some(parsed) = instruction.get("parsed") {
        if let Some(info) = parsed.get("info") {
            // Get token amounts for token-specific conditions
            if let Some(amount_str) = info.get("amount").and_then(|a| a.as_str()) {
                return amount_str.parse::<u64>().ok();
            }
            if let Some(amount_str) = info
                .get("tokenAmount")
                .and_then(|a| a.get("amount"))
                .and_then(|a| a.as_str())
            {
                return amount_str.parse::<u64>().ok();
            }
        }
    }
    None
}

fn check_buy_conditions(instructions: &[Value], account_keys: &[String]) -> bool {
    let instruction_types: Vec<String> = instructions
        .iter()
        .filter_map(get_instruction_type)
        .collect();

    // Get SOL amounts (for SOL/transfer conditions)
    let instruction_amounts: Vec<u64> = instructions
        .iter()
        .filter_map(get_instruction_amount)
        .collect();

    // Get token amounts (for token-specific conditions)
    let token_amounts: Vec<u64> = instructions.iter().filter_map(get_token_amount).collect();

    BUY_AFTER.iter().all(|required| {
        if required.starts_with("pubkey:") {
            // Check for specific pubkey presence
            let pubkey = &required[7..]; // Remove "pubkey:" prefix
            account_keys.iter().any(|key| key == pubkey)
        } else if required.starts_with("pubkey_any:") {
            // Check for any of the specified pubkeys (OR logic)
            let pubkeys = &required[11..]; // Remove "pubkey_any:" prefix
            let pubkey_list: Vec<&str> = pubkeys.split(',').collect();
            account_keys
                .iter()
                .any(|key| pubkey_list.contains(&key.as_str()))
        } else if required.starts_with("program:") {
            // Check for specific program involvement
            let program_id = &required[8..]; // Remove "program:" prefix
            instructions.iter().any(|ix| {
                ix.get("programId")
                    .and_then(|p| p.as_str())
                    .map_or(false, |p| p == program_id)
            })
        } else if required.starts_with("amount_over:") {
            // Check for SOL amount over threshold
            let threshold_str = &required[12..]; // Remove "amount_over:" prefix
            if let Ok(threshold) = threshold_str.parse::<u64>() {
                instruction_amounts.iter().any(|&amount| amount > threshold)
            } else {
                false
            }
        } else if required.starts_with("amount_under:") {
            // Check for SOL amount under threshold
            let threshold_str = &required[13..]; // Remove "amount_under:" prefix
            if let Ok(threshold) = threshold_str.parse::<u64>() {
                instruction_amounts.iter().any(|&amount| amount < threshold)
            } else {
                false
            }
        } else if required.starts_with("sol_over:") {
            // Check for SOL amount over threshold
            let threshold_str = &required[9..]; // Remove "sol_over:" prefix
            if let Ok(threshold_sol) = threshold_str.parse::<f64>() {
                let threshold_lamports = (threshold_sol * 1_000_000_000.0) as u64;
                instruction_amounts
                    .iter()
                    .any(|&amount| amount > threshold_lamports)
            } else {
                false
            }
        } else if required.starts_with("sol_under:") {
            // Check for SOL amount under threshold
            let threshold_str = &required[10..]; // Remove "sol_under:" prefix
            if let Ok(threshold_sol) = threshold_str.parse::<f64>() {
                let threshold_lamports = (threshold_sol * 1_000_000_000.0) as u64;
                instruction_amounts
                    .iter()
                    .any(|&amount| amount < threshold_lamports)
            } else {
                false
            }
        } else if required.starts_with("token_amount_over:") {
            // Check for token amount over threshold (uses token amounts)
            let threshold_str = &required[18..]; // Remove "token_amount_over:" prefix
            if let Ok(threshold) = threshold_str.parse::<u64>() {
                token_amounts.iter().any(|&amount| amount > threshold)
            } else {
                false
            }
        } else {
            // Default: check instruction types
            instruction_types.iter().any(|found| found == required)
        }
    })
}

async fn process_transaction_message(
    client: &Arc<RpcClient>,
    wallet: &Arc<solana_sdk::signer::keypair::Keypair>,
    transaction_data: &Value,
    mint_tracker: &Arc<RwLock<HashMap<String, u32>>>,
    pending_buys: &Arc<RwLock<HashMap<String, StoredTransaction>>>,
    bought_mints: &Arc<RwLock<HashSet<String>>>,
) -> Result<(), ProcessError> {
    let account_keys = transaction_data
        .get("transaction")
        .and_then(|t| t.get("transaction"))
        .and_then(|t| t.get("message"))
        .and_then(|m| m.get("accountKeys"))
        .and_then(|k| k.as_array())
        .ok_or("invalid transaction structure")?;

    let instructions = transaction_data
        .get("transaction")
        .and_then(|t| t.get("transaction"))
        .and_then(|t| t.get("message"))
        .and_then(|m| m.get("instructions"))
        .and_then(|i| i.as_array())
        .ok_or("invalid instructions structure")?;

    let signature = transaction_data
        .get("transaction")
        .and_then(|t| t.get("transaction"))
        .and_then(|t| t.get("signatures"))
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.get(0))
        .and_then(|s| s.as_str())
        .unwrap_or("unknown");

    let keys: Vec<String> = account_keys
        .iter()
        .filter_map(|k| k.get("pubkey").and_then(|p| p.as_str()))
        .map(|s| s.to_string())
        .collect();

    // Skip subsequent transaction logic if this is a mint creation transaction
    let is_mint_creation = keys.contains(&METAPLEX_KEY.to_string());

    if !is_mint_creation {
        // Check for subsequent transactions on tracked mints
        for key in &keys {
            let mut tracker = mint_tracker.write().await;
            if let Some(count) = tracker.get_mut(key) {
                if *count >= 1 && *count <= 4 {
                    *count += 1;
                    // Only check buy conditions on subsequent transactions (count > 1), not the creation tx
                    if *count > 1 {
                        async_log!(
                            "[{}] Checking conditions for mint: {}",
                            Utc::now().format("%H:%M:%S%.3f"),
                            key
                        );
                        if check_buy_conditions(instructions, &keys) {
                            async_log!(
                                "[{}] Conditions matched for mint: {}",
                                Utc::now().format("%H:%M:%S%.3f"),
                                key
                            );
                            let already_bought = bought_mints.read().await.contains(key);
                            if !already_bought {
                                if let Some(transaction) = pending_buys.write().await.remove(key) {
                                    async_log!(
                                        "[{}] Firing buy transaction for mint: {} with {:.6} SOL",
                                        Utc::now().format("%H:%M:%S%.3f"),
                                        key,
                                        SOL_AMOUNT_IN
                                    );
                                    // Spawn background task to spam buy attempts
                                    let client_clone = Arc::clone(client);
                                    let wallet_clone = Arc::clone(wallet);
                                    let bought_mints_clone = Arc::clone(bought_mints);
                                    let key_clone = key.clone();
                                    tokio::spawn(async move {
                                        for attempt in 0..MAX_RETRIES {
                                            match submit_stored_transaction(
                                                &client_clone,
                                                &wallet_clone,
                                                &transaction,
                                            )
                                            .await
                                            {
                                                Ok(result) => {
                                                    if USE_JITO {
                                                        async_log!("[{}] JITO bundle successful: {} for mint: {} with {:.6} SOL", Utc::now().format("%H:%M:%S%.3f"), result, key_clone, SOL_AMOUNT_IN);
                                                    } else {
                                                        async_log!("[{}] Buy successful: {} for mint: {} with {:.6} SOL", Utc::now().format("%H:%M:%S%.3f"), result, key_clone, SOL_AMOUNT_IN);
                                                    }
                                                    bought_mints_clone
                                                        .write()
                                                        .await
                                                        .insert(key_clone);
                                                    break;
                                                }
                                                Err(_) => {
                                                    if attempt < MAX_RETRIES - 1 {
                                                        tokio::time::sleep(Duration::from_millis(
                                                            50,
                                                        ))
                                                        .await;
                                                    }
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Check for new mint creation (contains metaplex program)
    if is_mint_creation {
        // Search for the mint creation instruction with "8c55d7" prefix
        for instruction in instructions {
            if let Some(data) = instruction.get("data").and_then(|d| d.as_str()) {
                // Decode base58 data to bytes and check if it starts with [0x8c, 0x55, 0xd7]
                if let Ok(decoded_data) = bs58::decode(data).into_vec() {
                    if decoded_data.len() >= 3 && decoded_data[0..3] == [0x8c, 0x55, 0xd7] {
                        // Found the mint creation instruction, get its accounts
                        if let Some(accounts) =
                            instruction.get("accounts").and_then(|a| a.as_array())
                        {
                            let instruction_keys: Vec<String> = accounts
                                .iter()
                                .filter_map(|acc| acc.as_str())
                                .map(|s| s.to_string())
                                .collect();

                            if instruction_keys.len() >= 16 {
                                // Use the mint from instruction accounts (typically at index 3)
                                let mint = instruction_keys[3].clone();

                                let already_bought = bought_mints.read().await.contains(&mint);
                                if !already_bought {
                                    mint_tracker.write().await.insert(mint.clone(), 1);

                                    async_log!(
                                        "[{}] Mint created: {} | tx: {}",
                                        Utc::now().format("%H:%M:%S%.3f"),
                                        mint,
                                        signature
                                    );
                                    async_log!(
                                        "[{}] Mint key: {}",
                                        Utc::now().format("%H:%M:%S%.3f"),
                                        instruction_keys[3]
                                    );

                                    if let Ok(transaction) =
                                        build_buy_transaction(client, wallet, &instruction_keys)
                                            .await
                                    {
                                        async_log!(
                                            "[{}] Built buy tx with keys: {:?}",
                                            Utc::now().format("%H:%M:%S%.3f"),
                                            instruction_keys
                                        );
                                        if USE_JITO {
                                            async_log!("[{}] Stored transaction for mint: {} with {:.6} SOL (JITO enabled)", Utc::now().format("%H:%M:%S%.3f"), mint, SOL_AMOUNT_IN);
                                        } else {
                                            async_log!("[{}] Stored transaction for mint: {} with {:.6} SOL (regular submission)", Utc::now().format("%H:%M:%S%.3f"), mint, SOL_AMOUNT_IN);
                                        }
                                        pending_buys.write().await.insert(mint, transaction);
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
        // Exit function after processing mint creation to prevent any further processing of this transaction
        return Ok(());
    }

    Ok(())
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rpc_url = "https://mainnet.helius-rpc.com/?api-key=27fd6baa-75e9-4d39-9832-d5a43419ad78";
    let ws_url = "wss://atlas-mainnet.helius-rpc.com/?api-key=27fd6baa-75e9-4d39-9832-d5a43419ad78";

    let client = Arc::new(RpcClient::new_with_commitment(
        rpc_url,
        CommitmentConfig::processed(),
    ));

    let wallet = Arc::new(Keypair::from_base58_string(
        "YOUR WALLET HERE",
    ));

    let mint_tracker = Arc::new(RwLock::new(HashMap::new()));
    let pending_buys = Arc::new(RwLock::new(HashMap::<String, StoredTransaction>::new()));
    let bought_mints = Arc::new(RwLock::new(HashSet::new()));
    println!("[{}] Sniper running", Utc::now().format("%H:%M:%S%.3f"));

    let url = Url::parse(ws_url)?;
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();
    pack_log_async!();
    let subscription_message = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "transactionSubscribe",
        "params": [
            {
                "failed": false,
                "accountInclude": TARGET_ACCOUNTS
            },
            {
                "commitment": "processed",
                "encoding": "jsonParsed",
                "transactionDetails": "full",
                "maxSupportedTransactionVersion": 0
            }
        ]
    });

    write
        .send(Message::Text(subscription_message.to_string()))
        .await?;

    let mut ping_interval = tokio::time::interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if let Err(_) = write.send(Message::Ping(vec![])).await {
                    break;
                }
            }
            message = read.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                            if let Some(result) = parsed.get("params").and_then(|p| p.get("result")) {
                                let client_clone = Arc::clone(&client);
                                let wallet_clone = Arc::clone(&wallet);
                                let tracker_clone = Arc::clone(&mint_tracker);
                                let pending_buys_clone = Arc::clone(&pending_buys);
                                let bought_mints_clone = Arc::clone(&bought_mints);
                                let result_clone = result.clone();

                                tokio::spawn(async move {
                                    let _ = process_transaction_message(
                                        &client_clone,
                                        &wallet_clone,
                                        &result_clone,
                                        &tracker_clone,
                                        &pending_buys_clone,
                                        &bought_mints_clone,
                                    )
                                    .await;
                                });
                            }
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(_)) => {
                        break;
                    }
                    None => {
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
