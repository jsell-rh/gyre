#!/bin/sh
# M26.2: Agent-side WireGuard mesh setup.
#
# Called from entrypoint.sh when GYRE_WG_ENABLED=true.
# Generates a WireGuard keypair, registers the pubkey with the server,
# fetches the peer list, and brings up wg0 with the allocated mesh IP.
#
# Requirements: wireguard-tools (wg, ip commands) must be installed in the image.
# The WireGuard interface creation requires NET_ADMIN capability.

set -eu

GYRE_SERVER_URL="${GYRE_SERVER_URL:-http://gyre-server:3000}"
GYRE_AUTH_TOKEN="${GYRE_AUTH_TOKEN:-}"
GYRE_AGENT_ID="${GYRE_AGENT_ID:-}"

if [ -z "$GYRE_AUTH_TOKEN" ] || [ -z "$GYRE_AGENT_ID" ]; then
    echo "[setup-wg] GYRE_AUTH_TOKEN or GYRE_AGENT_ID not set; skipping WireGuard setup"
    exit 0
fi

# Check if wg is available.
if ! command -v wg >/dev/null 2>&1; then
    echo "[setup-wg] wireguard-tools not installed; skipping WireGuard setup"
    exit 0
fi

WG_KEY_DIR="/etc/wireguard"
mkdir -p "$WG_KEY_DIR"
chmod 700 "$WG_KEY_DIR"

PRIVATE_KEY_FILE="$WG_KEY_DIR/privatekey"
PUBLIC_KEY_FILE="$WG_KEY_DIR/publickey"

# Generate keypair if not already present.
if [ ! -f "$PRIVATE_KEY_FILE" ]; then
    echo "[setup-wg] generating WireGuard keypair"
    wg genkey | tee "$PRIVATE_KEY_FILE" | wg pubkey > "$PUBLIC_KEY_FILE"
    chmod 600 "$PRIVATE_KEY_FILE"
fi

PUBKEY="$(cat "$PUBLIC_KEY_FILE")"
echo "[setup-wg] public key: $PUBKEY"

# Determine our public endpoint (best-effort using hostname).
ENDPOINT="${GYRE_WG_ENDPOINT:-$(hostname -I | awk '{print $1}'):51820}"

# Register pubkey with server; capture allocated mesh IP.
echo "[setup-wg] registering pubkey with server"
REGISTER_RESPONSE="$(curl -sfS -X POST \
    -H "Authorization: Bearer $GYRE_AUTH_TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"agent_id\":\"$GYRE_AGENT_ID\",\"wireguard_pubkey\":\"$PUBKEY\",\"endpoint\":\"$ENDPOINT\"}" \
    "$GYRE_SERVER_URL/api/v1/network/peers" 2>&1)" || {
    echo "[setup-wg] WARNING: failed to register WireGuard pubkey: $REGISTER_RESPONSE"
    exit 0
}

MESH_IP="$(printf '%s' "$REGISTER_RESPONSE" | grep -o '"mesh_ip":"[^"]*"' | cut -d'"' -f4)"
if [ -z "$MESH_IP" ]; then
    echo "[setup-wg] WARNING: no mesh_ip allocated (WireGuard may be disabled on server); skipping"
    exit 0
fi

echo "[setup-wg] allocated mesh IP: $MESH_IP"

# Bring up wg0 interface with the allocated mesh IP.
ip link add wg0 type wireguard 2>/dev/null || true
ip address add "$MESH_IP/16" dev wg0 2>/dev/null || true
wg set wg0 private-key "$PRIVATE_KEY_FILE"
ip link set wg0 up

echo "[setup-wg] wg0 interface up with address $MESH_IP/16"

# Fetch fresh peer list and add routes to all active peers.
echo "[setup-wg] fetching peer list"
PEERS="$(curl -sfS \
    -H "Authorization: Bearer $GYRE_AUTH_TOKEN" \
    "$GYRE_SERVER_URL/api/v1/network/peers" 2>&1)" || {
    echo "[setup-wg] WARNING: failed to fetch peer list: $PEERS"
    exit 0
}

# Parse peers (simple grep-based JSON extraction; jq is optional).
if command -v jq >/dev/null 2>&1; then
    echo "$PEERS" | jq -r '.[] | select(.agent_id != "'"$GYRE_AGENT_ID"'") | "\(.wireguard_pubkey) \(.endpoint // "") \(.mesh_ip // "")"' | \
    while IFS=' ' read -r peer_pubkey peer_endpoint peer_mesh_ip; do
        [ -z "$peer_pubkey" ] && continue
        echo "[setup-wg] adding peer $peer_pubkey endpoint=$peer_endpoint allowed=$peer_mesh_ip/32"
        if [ -n "$peer_endpoint" ] && [ -n "$peer_mesh_ip" ]; then
            wg set wg0 peer "$peer_pubkey" endpoint "$peer_endpoint" allowed-ips "$peer_mesh_ip/32"
        elif [ -n "$peer_mesh_ip" ]; then
            wg set wg0 peer "$peer_pubkey" allowed-ips "$peer_mesh_ip/32"
        fi
    done
else
    echo "[setup-wg] jq not available; skipping peer route setup (install jq for full mesh)"
fi

echo "[setup-wg] WireGuard mesh setup complete"
wg show wg0
