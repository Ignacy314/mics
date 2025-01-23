#!/bin/bash
. $HOME/.profile
. $HOME/.bashrc

while ! [ "$(ping -c 1 google.com)" ]; do
  sleep 1
done

git -C $HOME/andros/andros pull
just -f $HOME/andros/andros/justfile

sleep 2

echo "start andros" > $HOME/andros_started
while true; do andros; sleep 5; done
