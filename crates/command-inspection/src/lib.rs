//! command-inspection: responsibility-scoped agent tooling.
pub mod cmd_hash;
pub mod cmd_shape;
pub mod command;
pub mod shell;
pub mod simple_shell;

pub use command::{parse, ParsedCommand, Range};
