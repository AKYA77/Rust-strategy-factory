#!/bin/bash
# 24/7 Strategy Generation & Backtesting Loop
# Runs continuously, rotating through real historical data chunks
# Stop with Ctrl+C

set -e

FACTORY="/root/workspace/Rust-strategy-factory/target/release/rust-strategy-factory"
REGISTRY="/root/.local/share/rust-strategy-factory/state/research-real.sqlite"
CONFIG="/root/workspace/Rust-strategy-factory/configs/research-real-data-multi.json"

echo "=== Starting 24/7 Strategy Generation Loop ==="
echo "Factory: $FACTORY"
echo "Registry: $REGISTRY"
echo "Config: $CONFIG"
echo ""

# Ensure registry exists
if [ ! -f "$REGISTRY" ]; then
    echo "Creating new registry..."
    "$FACTORY" research "$CONFIG" "$REGISTRY" 1 > /dev/null 2>&1 || true
fi

ROUND=0
while true; do
    ROUND=$((ROUND + 1))
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Round $ROUND: Running 200 experiments on real data..."
    
    "$FACTORY" research "$CONFIG" "$REGISTRY" 200 2>&1 | tail -3
    
    # Show progress
    TOTAL=$(python3 -c "
import sqlite3
conn = sqlite3.connect('$REGISTRY')
print(conn.execute('SELECT COUNT(*) FROM experiments').fetchone()[0])
conn.close()
")
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Total experiments: $TOTAL"
    echo ""
    
    # Brief pause between rounds
    sleep 2
done
