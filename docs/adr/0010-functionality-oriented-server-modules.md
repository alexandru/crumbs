---
status: accepted
---

# Organize the server by functionality

Reorganize the entire `backend/server` crate around functionality modules so that a reader can find a functionality's implementation in one place. Each functionality owns its HTTP routes, handlers, MCP integration, and other functionality-specific definitions; shared definitions remain reusable across modules. The root route module composes functionality-owned routes, and the crate root retains server-wide composition and infrastructure.

The functionality modules are `custom_foods/`, `search/`, `oauth/`, and `home/`; `mcp/` owns shared MCP infrastructure. Search owns searching all datasets, while custom foods owns custom-food management. Feature-specific MCP tools live with their functionality.

Reusable definitions stay with their natural functionality owner where possible. Introduce a shared module only when the definitions have no clear functionality owner; do not introduce a general `shared/` directory.

This is a behavior-preserving refactor. HTTP paths and payloads, MCP tool names and schemas, authorization behavior, and the public Rust API remain unchanged; internal interfaces may change to establish module ownership.

Dependencies between server modules must be acyclic. Shared definitions remain with their natural owner only when this preserves the dependency direction. Composition code connects functionality modules to shared infrastructure without making that infrastructure import its consumers.

The root `routes.rs` registers functionality-owned HTTP routers and MCP tool groups, then passes tools to shared MCP infrastructure. Shared MCP infrastructure does not import the functionality modules that provide its tools. Adding a functionality requires root registration; adding a tool to an existing functionality stays within that functionality.

Each folder exposes its interface through `mod.rs`; implementation submodules remain private. Root composition calls functionality exports rather than reaching into implementation files.

Shared static asset serving belongs to `assets/`, composed by the root route module. The stylesheet shared by the welcome and OAuth pages is named `main.css`.

Pages load `/assets/main.css`; `/assets/login.css` remains an alias for the same stylesheet to preserve the existing URL.
