# Agent Runtime & Compute

## Compute Targets

Gyre must support launching secure agents on **any compute target**, including but not limited to:

- Remote machines (SSH, etc.)
- Kubernetes / OpenShift
- Local containers (Docker, Podman)

The compute layer is **pluggable** - Gyre should not assume containers or orchestrators. A bare VM or local process is a valid target.

## Agent Interface Requirements

- **Full TTY support** - agents must have interactive terminal access.
- **Full WebSocket communication** between server and agent runners - the primary transport layer.
- **UI can attach to agent TTY** - users can observe and interact with running agents directly from the browser.
- Remote instances should have **configurable lifetimes** - bounded execution prevents runaway agents and resource leaks.

## Agent Invocation

- Agents are launched via a **cross-platform CLI command**.
- When a runtime (remote or local) is provisioned, Gyre runs the CLI with options as specified by the user.
- The CLI is the single entry point - same binary/command regardless of compute target.
- CLI includes a **TUI** for managing agents, viewing status, attaching to sessions, etc.

## Networking & Connectivity

Zero network connectivity is assumed at provision time. Gyre solves this with a **WireGuard mesh** (Tailscale-based):

1. **Server is the broker.** It runs Tailscale and maintains the DERP map for all WireGuard public keys.
2. **Provisioning flow:**
   - Cloud-init customization variables are passed to kernel/container at boot.
   - A SPIFFE boot identity is injected during provisioning via API.
   - On first boot, the remote uses its SPIFFE identity to broker WireGuard connectivity back to the server.
3. **The CLI binary is also a WireGuard client.** This gives you **3-way connectivity:**
   - Developer machine ↔ remote agent (direct)
   - Remote agent ↔ server (brokered via WireGuard/Tailscale)
   - Developer machine ↔ server
4. **Isolation:** Every remote/server/agent connection is isolated from one another.
5. **Port forwarding:** Forward a running web server on a remote to your local machine for testing. Or forward between agent remotes (agent-to-agent connectivity).

This also answers **"how do you repair a remote?"** - you have network connectivity to it via the mesh. The CLI gives you a direct tunnel.

## IDE of the Future

- Still need to support the **IDE of today** for today's workflows.
- But the networking mesh + TTY attach + port forwarding starts to define what a future IDE looks like - the remote *is* the dev environment, and you connect into it.

## Agent Protocols

| Protocol | Status | Usage |
|---|---|---|
| **MCP** | Adopt | Tool/data access. MCP servers defined server-side, injected to agents on spawn. |
| **A2A** | Adopt | Agent discovery and inter-agent communication. Agents publish Agent Cards describing capabilities. Manager discovers and routes. Enables external agent interop. |
| **AG-UI event taxonomy** | Borrow | Typed event stream vocabulary (TOOL_CALL_START, TEXT_MESSAGE_CONTENT, RUN_FINISHED, etc.) for WebSocket comms, TTY attach, activity feed, and audit trail. Transport is WebSocket, not SSE. |
| **AP2 mandate pattern** | Inspire | Typed authorization flow (intent → signed mandate → receipt) as a model for Gyre's approval and impersonation audit trails. Don't adopt the protocol; steal the pattern. |

## MCP Server Integration

- **MCP servers defined server-side** - agents automatically get access on spawn, no client-side config.
- Delivery mechanism TBD:
  - **Proxy through Gyre server**, or
  - **Direct injection** into the agent runtime environment
- Either way, the source of truth for MCP config lives on the server.
