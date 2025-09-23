extract-rsrc:
    cd rsrc && for f in *.tar.zst; do tar xvzf $f; done