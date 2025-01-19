#!/bin/sh

mkdir -p ~/project
cd ~/project
mkdir -p data
mkdir -p data/{i2s,umc}
mkdir -p log
git clone https://github.com/Ignacy314/mics

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cd mics
cargo build -r
