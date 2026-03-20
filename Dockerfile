FROM ghcr.io/hypermemetic/claude-container:latest

RUN apt-get update && apt-get install -y --no-install-recommends \
    sudo \
    tmux \
    asciinema \
    && rm -rf /var/lib/apt/lists/*

# Pre-built Linux binaries (extracted from locus-cast-test multi-stage build)
COPY bin/synapse /usr/local/bin/synapse
COPY bin/plexus-locus /usr/local/bin/plexus-locus
