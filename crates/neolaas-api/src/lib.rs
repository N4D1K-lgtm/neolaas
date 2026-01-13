//! API module for neolaas
//!
//! REST API handlers and routing logic

pub mod routes;
pub mod handlers;

// Re-export commonly used items
pub use routes::build_router;
