//! MCP (Model Context Protocol) transport and tool handlers.
//!
//! Implements stdio-based JSON-RPC transport for the MCP protocol.
//! Registers 42 tools across indexing, search, graph, memory, context, utility, and archaeology categories.

pub mod protocol;
pub mod tools;
pub mod transport;
pub mod architecture;

pub use transport::McpTransport;
pub use tools::ToolRegistry;
