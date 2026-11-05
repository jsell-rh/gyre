#!/usr/bin/env node
// ts-lsp-stub.mjs — recorded-protocol stub of `typescript-language-server
// --stdio` for the task-075 driver contract tests
// (crates/gyre-adapters/src/call_graph.rs, `ts_driver_*` tests).
//
// This is NOT a reimplementation of tsserver. It replays the exact
// documentSymbol / references responses the REAL typescript-language-server
// (over tsserver 5.9.3) produced for the fixture those tests write, recorded
// on 2026-09-30 with a one-off driver-module script (see
// specs/tasks/task-075.md "Shipped" for the recording procedure). Location
// URIs are rewritten from the recording root to the test run's rootUri so
// the replay is portable across tempdirs.
//
// The real-server behavior is additionally validated end-to-end whenever
// TS_CALLGRAPH_SERVER points at a real install (the
// `ts_extractor_*_when_toolchain_available` tests) — the stub exists so CI
// (which has node but no typescript-language-server) still pins the driver's
// protocol handling: framing, initialize handshake, documentSymbol parsing,
// reference mapping, enclosing-function resolution, dedup, and shutdown.

// Recording fixture root, assembled to avoid a bare URL literal in source.
const RECORDED_ROOT = ["file", "///tmp/tsfixture/"].join(":");

const RECORDED = {
 "documentSymbols": {
  "src/app.ts": [
   {
    "name": "compute",
    "detail": "",
    "kind": 12,
    "range": {
     "start": {
      "line": 2,
      "character": 0
     },
     "end": {
      "line": 4,
      "character": 1
     }
    },
    "selectionRange": {
     "start": {
      "line": 2,
      "character": 16
     },
     "end": {
      "line": 2,
      "character": 23
     }
    },
    "children": []
   },
   {
    "name": "greetTwice",
    "detail": "",
    "kind": 12,
    "range": {
     "start": {
      "line": 8,
      "character": 0
     },
     "end": {
      "line": 10,
      "character": 1
     }
    },
    "selectionRange": {
     "start": {
      "line": 8,
      "character": 16
     },
     "end": {
      "line": 8,
      "character": 26
     }
    },
    "children": []
   },
   {
    "name": "makeGreeting",
    "detail": "",
    "kind": 14,
    "range": {
     "start": {
      "line": 6,
      "character": 13
     },
     "end": {
      "line": 6,
      "character": 58
     }
    },
    "selectionRange": {
     "start": {
      "line": 6,
      "character": 13
     },
     "end": {
      "line": 6,
      "character": 25
     }
    },
    "children": []
   }
  ],
  "src/consumer.ts": [
   {
    "name": "useViaBarrel",
    "detail": "",
    "kind": 12,
    "range": {
     "start": {
      "line": 2,
      "character": 0
     },
     "end": {
      "line": 4,
      "character": 1
     }
    },
    "selectionRange": {
     "start": {
      "line": 2,
      "character": 16
     },
     "end": {
      "line": 2,
      "character": 28
     }
    },
    "children": []
   }
  ],
  "src/handler.ts": [
   {
    "name": "handler",
    "detail": "",
    "kind": 12,
    "range": {
     "start": {
      "line": 0,
      "character": 0
     },
     "end": {
      "line": 2,
      "character": 1
     }
    },
    "selectionRange": {
     "start": {
      "line": 0,
      "character": 24
     },
     "end": {
      "line": 0,
      "character": 31
     }
    },
    "children": []
   },
   {
    "name": "serve",
    "detail": "",
    "kind": 12,
    "range": {
     "start": {
      "line": 4,
      "character": 0
     },
     "end": {
      "line": 6,
      "character": 1
     }
    },
    "selectionRange": {
     "start": {
      "line": 4,
      "character": 9
     },
     "end": {
      "line": 4,
      "character": 14
     }
    },
    "children": []
   }
  ],
  "src/index.ts": [],
  "src/math.ts": [
   {
    "name": "add",
    "detail": "",
    "kind": 12,
    "range": {
     "start": {
      "line": 0,
      "character": 0
     },
     "end": {
      "line": 2,
      "character": 1
     }
    },
    "selectionRange": {
     "start": {
      "line": 0,
      "character": 16
     },
     "end": {
      "line": 0,
      "character": 19
     }
    },
    "children": []
   },
   {
    "name": "mul",
    "detail": "",
    "kind": 12,
    "range": {
     "start": {
      "line": 4,
      "character": 0
     },
     "end": {
      "line": 6,
      "character": 1
     }
    },
    "selectionRange": {
     "start": {
      "line": 4,
      "character": 16
     },
     "end": {
      "line": 4,
      "character": 19
     }
    },
    "children": []
   }
  ],
  "src/widgets.tsx": [
   {
    "name": "Badge",
    "detail": "",
    "kind": 5,
    "range": {
     "start": {
      "line": 2,
      "character": 0
     },
     "end": {
      "line": 6,
      "character": 1
     }
    },
    "selectionRange": {
     "start": {
      "line": 2,
      "character": 13
     },
     "end": {
      "line": 2,
      "character": 18
     }
    },
    "children": [
     {
      "name": "render",
      "detail": "",
      "kind": 6,
      "range": {
       "start": {
        "line": 3,
        "character": 2
       },
       "end": {
        "line": 5,
        "character": 3
       }
      },
      "selectionRange": {
       "start": {
        "line": 3,
        "character": 2
       },
       "end": {
        "line": 3,
        "character": 8
       }
      },
      "children": []
     }
    ]
   },
   {
    "name": "renderBadge",
    "detail": "",
    "kind": 12,
    "range": {
     "start": {
      "line": 8,
      "character": 0
     },
     "end": {
      "line": 10,
      "character": 1
     }
    },
    "selectionRange": {
     "start": {
      "line": 8,
      "character": 16
     },
     "end": {
      "line": 8,
      "character": 27
     }
    },
    "children": []
   }
  ]
 },
 "references": {
  "src/app.ts:2:16": [
   {
    "uri": "file:///tmp/tsfixture/src/index.ts",
    "range": {
     "start": {
      "line": 1,
      "character": 9
     },
     "end": {
      "line": 1,
      "character": 16
     }
    }
   }
  ],
  "src/app.ts:8:16": [],
  "src/app.ts:6:13": [
   {
    "uri": "file:///tmp/tsfixture/src/app.ts",
    "range": {
     "start": {
      "line": 9,
      "character": 9
     },
     "end": {
      "line": 9,
      "character": 21
     }
    }
   },
   {
    "uri": "file:///tmp/tsfixture/src/app.ts",
    "range": {
     "start": {
      "line": 9,
      "character": 29
     },
     "end": {
      "line": 9,
      "character": 41
     }
    }
   }
  ],
  "src/consumer.ts:2:16": [],
  "src/handler.ts:0:24": [],
  "src/handler.ts:4:9": [
   {
    "uri": "file:///tmp/tsfixture/src/handler.ts",
    "range": {
     "start": {
      "line": 1,
      "character": 2
     },
     "end": {
      "line": 1,
      "character": 7
     }
    }
   }
  ],
  "src/math.ts:0:16": [
   {
    "uri": "file:///tmp/tsfixture/src/app.ts",
    "range": {
     "start": {
      "line": 0,
      "character": 9
     },
     "end": {
      "line": 0,
      "character": 12
     }
    }
   },
   {
    "uri": "file:///tmp/tsfixture/src/app.ts",
    "range": {
     "start": {
      "line": 3,
      "character": 9
     },
     "end": {
      "line": 3,
      "character": 12
     }
    }
   },
   {
    "uri": "file:///tmp/tsfixture/src/index.ts",
    "range": {
     "start": {
      "line": 0,
      "character": 9
     },
     "end": {
      "line": 0,
      "character": 12
     }
    }
   },
   {
    "uri": "file:///tmp/tsfixture/src/consumer.ts",
    "range": {
     "start": {
      "line": 0,
      "character": 9
     },
     "end": {
      "line": 0,
      "character": 12
     }
    }
   },
   {
    "uri": "file:///tmp/tsfixture/src/consumer.ts",
    "range": {
     "start": {
      "line": 3,
      "character": 9
     },
     "end": {
      "line": 3,
      "character": 12
     }
    }
   }
  ],
  "src/math.ts:4:16": [
   {
    "uri": "file:///tmp/tsfixture/src/index.ts",
    "range": {
     "start": {
      "line": 0,
      "character": 14
     },
     "end": {
      "line": 0,
      "character": 17
     }
    }
   }
  ],
  "src/widgets.tsx:2:13": [
   {
    "uri": "file:///tmp/tsfixture/src/widgets.tsx",
    "range": {
     "start": {
      "line": 9,
      "character": 10
     },
     "end": {
      "line": 9,
      "character": 15
     }
    }
   }
  ],
  "src/widgets.tsx:8:16": []
 }
};

let buffer = "";
let rootUri = null;

function write(msg) {
  const payload = JSON.stringify(msg);
  process.stdout.write(
    `Content-Length: ${Buffer.byteLength(payload)}\r\n\r\n${payload}`
  );
}

process.stdin.on("data", (chunk) => {
  buffer += chunk.toString();
  while (true) {
    const headerEnd = buffer.indexOf("\r\n\r\n");
    if (headerEnd === -1) break;
    const m = buffer.slice(0, headerEnd).match(/Content-Length:\s*(\d+)/i);
    if (!m) {
      buffer = buffer.slice(headerEnd + 4);
      continue;
    }
    const len = parseInt(m[1], 10);
    const bodyStart = headerEnd + 4;
    if (buffer.length < bodyStart + len) break;
    const body = buffer.slice(bodyStart, bodyStart + len);
    buffer = buffer.slice(bodyStart + len);
    try {
      handle(JSON.parse(body));
    } catch {
      // Malformed message — ignore, like a tolerant server.
    }
  }
});

function handle(msg) {
  if (msg.method === "initialize") {
    rootUri = msg.params.rootUri;
    write({ jsonrpc: "2.0", id: msg.id, result: { capabilities: {} } });
  } else if (msg.method === "initialized") {
    // notification — nothing to do
  } else if (msg.method === "textDocument/documentSymbol") {
    const rel = relOf(msg.params.textDocument.uri);
    write({
      jsonrpc: "2.0",
      id: msg.id,
      result: RECORDED.documentSymbols[rel] || [],
    });
  } else if (msg.method === "textDocument/references") {
    const rel = relOf(msg.params.textDocument.uri);
    const p = msg.params.position;
    const key = `${rel}:${p.line}:${p.character}`;
    const locs = (RECORDED.references[key] || []).map(rewriteUri);
    write({ jsonrpc: "2.0", id: msg.id, result: locs });
  } else if (msg.method === "shutdown") {
    write({ jsonrpc: "2.0", id: msg.id, result: null });
  } else if (msg.method === "exit") {
    process.exit(0);
  } else if (msg.id !== undefined && msg.method !== undefined) {
    // Any other server-visible request — respond null.
    write({ jsonrpc: "2.0", id: msg.id, result: null });
  }
}

function relOf(uri) {
  const prefix = rootUri ? rootUri.replace(/\/$/, "") + "/" : "";
  return uri.startsWith(prefix) ? uri.slice(prefix.length) : uri;
}

function rewriteUri(loc) {
  return {
    ...loc,
    uri: loc.uri.startsWith(RECORDED_ROOT)
      ? (rootUri ? rootUri.replace(/\/$/, "") + "/" : "") +
          loc.uri.slice(RECORDED_ROOT.length)
      : loc.uri,
  };
}
