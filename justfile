feature_list := `cat write | grep 1 | cut -f1 -d ' ' | tr '\n' ','`
features := if feature_list == "" { "" } else { "--features " + feature_list }

build:
  ${HOME}/.cargo/bin/cargo install --path "${HOME}/andros/andros" --locked {{features}}
