#!/usr/bin/env python3
"""Fake rust-analyzer LSP server for hermetic Rust Pass 2 tests.

Stands in for the real `rust-analyzer` binary (via the RUST_ANALYZER_BIN
override) so the adapter's LSP protocol handling - initialize handshake,
workspaceLoaded wait, didOpen, textDocument/references with exact
positions, shutdown/exit - is exercised end-to-end in CI without the real
toolchain.

Usage:
    fake-rust-analyzer.py <refs.json> <requests.log>

refs.json maps a query key "<uri>|<line>|<character>" (0-based LSP position
of the definition identifier) to the list of reference locations the server
should report: [{"uri": ..., "range": {"start": {"line": ..., "character":
0}, "end": {"line": ..., "character": 0}}}, ...]

requests.log receives one JSON object per received message with a "method"
field: {"method": ..., "id": ..., "params": ...}. Tests assert the exact
requests the adapter made (see call_graph_rust.rs tests).
"""

import json
import sys


def read_msg(stdin):
    """Read one Content-Length framed JSON-RPC message, or None on EOF."""
    content_length = None
    while True:
        line = stdin.readline()
        if not line:
            return None
        stripped = line.strip()
        if not stripped:
            break  # end of headers
        name, _, value = stripped.partition(b":")
        if name.strip().lower() == b"content-length":
            content_length = int(value.strip())
    if content_length is None:
        return None
    body = stdin.read(content_length)
    return json.loads(body)


def write_msg(stdout, msg):
    body = json.dumps(msg).encode("utf-8")
    stdout.write(b"Content-Length: " + str(len(body)).encode() + b"\r\n\r\n")
    stdout.write(body)
    stdout.flush()


def main():
    if len(sys.argv) != 3:
        sys.stderr.write("usage: fake-rust-analyzer.py <refs.json> <requests.log>\n")
        sys.exit(2)

    with open(sys.argv[1], "r", encoding="utf-8") as f:
        refs = json.load(f)
    log = open(sys.argv[2], "a", encoding="utf-8")

    stdin = sys.stdin.buffer
    stdout = sys.stdout.buffer

    while True:
        msg = read_msg(stdin)
        if msg is None:
            break
        method = msg.get("method")
        if method is None:
            continue  # response to a server-initiated request; none are sent
        msg_id = msg.get("id")
        params = msg.get("params")

        log.write(json.dumps({"method": method, "id": msg_id, "params": params}) + "\n")
        log.flush()

        if method == "initialize":
            write_msg(
                stdout,
                {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {"capabilities": {}},
                },
            )
        elif method == "initialized":
            # Mirror rust-analyzer's custom workspace-load signal.
            write_msg(
                stdout,
                {
                    "jsonrpc": "2.0",
                    "method": "rust-analyzer/workspaceLoaded",
                    "params": None,
                },
            )
        elif method == "textDocument/references":
            text_doc = params.get("textDocument", {})
            position = params.get("position", {})
            key = "{}|{}|{}".format(
                text_doc.get("uri", ""),
                position.get("line", -1),
                position.get("character", -1),
            )
            write_msg(
                stdout,
                {"jsonrpc": "2.0", "id": msg_id, "result": refs.get(key, [])},
            )
        elif method == "shutdown":
            write_msg(stdout, {"jsonrpc": "2.0", "id": msg_id, "result": None})
        elif method == "exit":
            log.close()
            sys.exit(0)
        elif msg_id is not None:
            # Any other server-visible request: acknowledge with null.
            write_msg(stdout, {"jsonrpc": "2.0", "id": msg_id, "result": None})

    log.close()
    sys.exit(0)


if __name__ == "__main__":
    main()
