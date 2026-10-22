#!/usr/bin/env bash
# One generation of Kishi's reinforcement loop:
#
#   scripts/generation.sh N [positions] [epochs]
#
#   1. train nets/genN.nnue on the self-play data of the last three generations
#   2. play it against the previous generation and log the result
#   3. let it play itself to produce data/genN.bin for the next generation
#
# Generation 0 is the hand-written evaluation; its games are data/gen0.bin.
set -euo pipefail
cd "$(dirname "$0")/.."

n=$1
positions=${2:-10000000}
epochs=${3:-30}
prev=$([ "$n" -eq 1 ] && echo hce || echo "nets/gen$((n - 1)).nnue")

data=$(for g in $(seq $((n > 3 ? n - 3 : 0)) $((n - 1))); do [ -f "data/gen$g.bin" ] && printf 'data/gen%d.bin,' "$g"; done)
data=${data%,}

cargo build --release -p kishi-tools
echo "== gen $n: training on $data"
./target/release/train --data "$data" --out "nets/gen$n.nnue" --epochs "$epochs" 2>&1 | tee "data/train$n.log"

echo "== gen $n: match against $prev"
result=$(./target/release/arena --a "nets/gen$n.nnue" --b "$prev" --games 400 --nodes 20000 | tail -1)
echo "$result"
echo "- $(date +%F) gen $n, trained on ${data//,/ }: $result" >> nets/LOG.md

echo "== gen $n: self-play"
./target/release/datagen --brain "nets/gen$n.nnue" --out "data/gen$n.bin" --positions "$positions" --seed "$((n * 1000))" 2>&1 | tail -1
