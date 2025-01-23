#!/bin/bash
while ! [ "$(ping -c 1 google.com)" ]; do
  sleep 1
done

# cd $HOME/andros/andros && /usr/bin/git pull
/usr/bin/git -C $HOME/andros/andros pull

# if [ $(cat $HOME/andros/andros/write_to_disk) = 1 ]; then
#   $HOME/.cargo/bin/cargo install --path \$HOME/andros/andros --locked
# else
#   $HOME/.cargo/bin/cargo install --path \$HOME/andros/andros --locked --no-default-features
# fi

$HOME/andros/andros/justfile

sleep 2

echo "start andros" > $HOME/andros_started
while true; do /home/test/.cargo/bin/andros; sleep 5; done
