//! MCP (Model Context Protocol) transport and tool handlers.
//!
//! Implements stdio-based JSON-RPC transport for the MCP protocol.
//! Registers 42 tools across indexing, search, graph, memory, context, utility, and archaeology categories.

pub mod architecture;
pub mod protocol;
pub mod tools;
pub mod transport;

pub use tools::ToolRegistry;
pub use transport::McpTransport;
