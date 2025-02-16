#feature_list := `cat write | grep 1 | cut -f1 -d ' ' | tr '\n' ','`
#features_from_file := if feature_list == "" { "" } else { "--features " + feature_list }

build features="audio,sensors":
  ${HOME}/.cargo/bin/cargo install --path "${HOME}/andros/andros" --locked --no-default-features {{ if features == "none" { "" } else { "--features " + features } }}
