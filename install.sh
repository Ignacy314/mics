#!/bin/sh

mkdir -p ~/project
cd ~/project
mkdir -p data
mkdir -p data/data
mkdir -p data/i2s
mkdir -p data/umc
mkdir -p log
rm -rf mics
git clone https://github.com/Ignacy314/mics

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
apt-get install -y libasound2-dev;
apt-get install -y libwebkit2gtk-4.0;
# sudo apt-get install cmake;
# sudo apt-get install gfortran;
cd mics
cp ANDROSi2s.dtbo /boot/firmware/overlays
cp -f config.txt /boot/firmware/config.txt
chmod +x run.sh
ln -s ~/project/mics/run.sh /usr/local/bin/andros
# $SHELL
# cargo build -r
