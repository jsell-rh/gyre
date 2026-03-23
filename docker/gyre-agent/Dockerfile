FROM ubuntu:22.04

RUN apt-get update && apt-get install -y \
    git curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create workspace directory
RUN mkdir -p /gyre /workspace

# Copy entrypoint script
COPY entrypoint.sh /gyre/entrypoint.sh
RUN chmod +x /gyre/entrypoint.sh

WORKDIR /workspace
ENTRYPOINT ["/gyre/entrypoint.sh"]
