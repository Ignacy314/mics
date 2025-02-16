#!/bin/bash
. $HOME/.profile
. $HOME/.bashrc
just -f $HOME/andros/andros/justfile build $1
andros
