#!/bin/bash
set -e

echo "Starting comprehensive compilation error fix script..."

# 1. Fix the TrackedReceiver type mismatch in signers_actor.rs
echo "Fixing TrackedReceiver type mismatch in signers_actor.rs..."
if grep -q "let mut compose_channel_rx: Receiver<MessageTxCompose<LDT>> = compose_channel_rx.subscribe()" crates/broadcast/accounts/src/signers/signers_actor.rs; then
  sed -i 's/let mut compose_channel_rx: Receiver<MessageTxCompose<LDT>> = compose_channel_rx.subscribe()/let mut compose_channel_rx = compose_channel_rx.subscribe()/' crates/broadcast/accounts/src/signers/signers_actor.rs
  echo "Fixed type mismatch in signers_actor.rs"
fi

# 2. Fix unused mutable variable in blockchain-shared/lib.rs
echo "Fixing unused mutable variable in blockchain-shared/lib.rs..."
if grep -q "let mut market_instance = Market::default()" crates/core/blockchain-shared/src/lib.rs; then
  sed -i 's/let mut market_instance = Market::default()/let market_instance = Market::default()/' crates/core/blockchain-shared/src/lib.rs
  echo "Fixed unused mutable variable in blockchain-shared/lib.rs"
fi

# 3. Check for any other similar issues in the codebase
echo "Checking for other similar issues..."

# Look for other TrackedReceiver mismatches
grep -r "let.*: Receiver<.*> = .*\.subscribe()" --include="*.rs" . || echo "No other TrackedReceiver mismatches found"

# Look for other unused mutable variables (this is just informational, not fixing automatically)
echo "Checking for other unused mutable variables (informational only)..."
grep -r "let mut" --include="*.rs" . | grep -v "if let mut" | head -n 10 || echo "No other potential unused mutable variables found"

echo "All fixes applied successfully!"
echo "Please rebuild the application with: cargo build --release"