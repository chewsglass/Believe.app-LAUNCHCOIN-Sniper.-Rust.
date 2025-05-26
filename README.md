# Believe.app Sniper Bot

A high-performance Solana token sniper bot designed to automatically detect new token launches on the Believe.app platform and execute buy orders based on configurable conditions.

## 🚀 Features

- **Real-time Token Detection**: Monitors Solana blockchain for new token mints via WebSocket
- **Configurable Buy Conditions**: Multiple trigger conditions for automated buying
- **JITO Bundle Support**: Optional MEV protection through JITO bundles
- **High-Speed Execution**: Sub-second transaction submission with retry mechanisms
- **Flexible Configuration**: Customizable buy amounts, slippage, and trigger conditions

## ⚙️ Configuration

### User Settings (Modify these in the code)

```rust
const USE_JITO: bool = true;                    // Enable JITO bundle submission
const SOL_AMOUNT_IN: f64 = 0.0001;             // SOL amount to spend per buy
const MIN_TOKENS_OUT: u64 = 1111111;           // Minimum tokens expected
const COMPUTE_UNIT_PRICE: u64 = 1000000;       // Gas price in micro-lamports
const TIP_AMOUNT: u64 = 100000;                // JITO tip amount (0.0001 SOL)
```

### Buy Conditions

Configure when the bot should execute buy orders using the `BUY_AFTER` array:

```rust
const BUY_AFTER: &[&str] = &["transfer"];
```

#### Available Conditions:

| Condition | Description | Example |
|-----------|-------------|---------|
| `"transfer"` | Wait for any transfer instruction | `&["transfer"]` |
| `"transferChecked"` | Wait for transferChecked instruction | `&["transferChecked"]` |
| `"swap"` | Wait for swap instruction | `&["swap"]` |
| `"pubkey:ADDRESS"` | Wait for specific wallet involvement | `&["pubkey:So11111111111111111111111111111111111111112"]` |
| `"pubkey_any:ADDR1,ADDR2"` | Wait for ANY of these wallets (OR logic) | `&["pubkey_any:WALLET1,WALLET2"]` |
| `"program:PROGRAM_ID"` | Wait for specific program usage | `&["program:11111111111111111111111111111111"]` |
| `"sol_over:AMOUNT"` | Wait for SOL transfer over amount | `&["sol_over:0.1"]` |
| `"sol_under:AMOUNT"` | Wait for SOL transfer under amount | `&["sol_under:1.0"]` |
| `"token_amount_over:AMOUNT"` | Wait for token transfer over amount | `&["token_amount_over:1000"]` |

#### Condition Examples:

```rust
// Buy after any transfer
const BUY_AFTER: &[&str] = &["transfer"];

// Buy when both transfer AND transferChecked occur
const BUY_AFTER: &[&str] = &["transfer", "transferChecked"];

// Buy when whale wallet is involved
const BUY_AFTER: &[&str] = &["transfer", "pubkey:WHALE_WALLET_ADDRESS"];

// Buy when SOL transfer over 0.5 SOL occurs
const BUY_AFTER: &[&str] = &["transfer", "sol_over:0.5"];

// Buy when any of multiple wallets are involved
const BUY_AFTER: &[&str] = &["pubkey_any:WALLET1,WALLET2,WALLET3"];
```

## 🛠️ Setup

### Prerequisites

- Rust (latest stable version)
- Solana CLI tools
- RPC endpoint with WebSocket support (Helius recommended)

### Installation

1. **Clone the repository**
```bash
git clone <repository-url>
cd believe-sniper
```

2. **Install dependencies**
```bash
cargo build --release
```

3. **Configure your wallet**
```rust
let wallet = Arc::new(Keypair::from_base58_string(
    "YOUR_PRIVATE_KEY_HERE",
));
```

4. **Set your RPC endpoints**
```rust
let rpc_url = "https://mainnet.helius-rpc.com/?api-key=YOUR_API_KEY";
let ws_url = "wss://atlas-mainnet.helius-rpc.com/?api-key=YOUR_API_KEY";
```

### Running the Bot

```bash
cargo run --release
```

## 🔧 How It Works

### 1. **Token Detection**
- Monitors transactions involving Believe.app program accounts
- Detects new token mints by looking for Metaplex instructions with `8c55d7` prefix
- Extracts token mint address and relevant account keys

### 2. **Transaction Building**
- Pre-builds buy transactions for detected tokens
- Creates associated token accounts for WSOL and new token
- Constructs swap instruction with configured parameters

### 3. **Condition Monitoring**
- Tracks subsequent transactions for each detected mint
- Evaluates configured buy conditions against transaction data
- Triggers buy execution when all conditions are met

### 4. **Execution**
- Submits transactions via JITO bundles (if enabled) or regular RPC
- Implements retry mechanism with configurable attempts
- Provides detailed logging for monitoring and debugging

## 📊 Transaction Flow

```
1. New Token Detected → Build Buy Transaction → Store in Memory
2. Monitor Subsequent Transactions → Check Buy Conditions
3. Conditions Met → Execute Buy Transaction → Retry Until Success
4. Success → Mark Token as Bought → Continue Monitoring
```

## ⚠️ Risk Warnings

- **Financial Risk**: Automated trading involves significant financial risk
- **Slippage**: High-speed trading may result in unfavorable prices
- **Failed Transactions**: Network congestion can cause transaction failures
- **Smart Contract Risk**: Interacting with unaudited contracts is risky
- **Regulatory Risk**: Ensure compliance with local regulations

## 🔒 Security Best Practices

- **Private Keys**: Never share or commit private keys to version control
- **RPC Security**: Use secure, authenticated RPC endpoints
- **Amount Limits**: Start with small amounts for testing
- **Monitoring**: Always monitor bot activity and performance
- **Backup**: Keep secure backups of your wallet

## 📈 Performance Optimization

### JITO Bundles
- Enable `USE_JITO: true` for MEV protection
- Adjust `TIP_AMOUNT` based on network congestion
- Monitor bundle success rates

### Gas Optimization
- Tune `COMPUTE_UNIT_PRICE` for faster execution
- Balance speed vs. cost based on market conditions

### Network Settings
- Use high-performance RPC providers (Helius, QuickNode)
- Consider geographic proximity to reduce latency

## 🐛 Troubleshooting

### Common Issues

**Connection Errors**
- Check RPC endpoint availability
- Verify API key validity
- Ensure WebSocket connection stability

**Transaction Failures**
- Increase `COMPUTE_UNIT_PRICE` for priority
- Check wallet SOL balance
- Verify slippage settings

**No Tokens Detected**
- Confirm target accounts are correct
- Check WebSocket subscription status
- Verify Believe.app program activity