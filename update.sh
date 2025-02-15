#!/bin/bash
echo "update start"
. $HOME/.profile
. $HOME/.bashrc

echo "waiting for internet connection..."
while ! [ "$(ping -c 1 10.66.66.1)" ]; do
  sleep 1
done

echo "checking for updates"
git -C $HOME/andros/andros pull
just -f $HOME/andros/andros/justfile

# sleep 2

echo "starting andros"
echo "start andros" > $HOME/started_andros
while true; do andros; sleep 5; done
