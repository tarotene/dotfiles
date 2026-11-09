//! agent-session: responsibility-scoped agent tooling.
pub mod cli;
pub mod decision;
pub mod entry;
pub mod escape;
pub mod event;
pub mod gate_event;
pub mod hook;
pub mod input;
pub mod ledger;
pub mod outcome;
pub mod plan;
pub mod reply;
pub mod stop;
pub mod transcript;

pub use decision::{Decision, PermissionDecision};
pub use event::HookEvent;
pub use input::{Agent, HookInput};
pub use ledger::SessionLedger;
pub use outcome::{Fault, Outcome, Reentry, StopOutcome, StopResult};
pub use reply::Reply;
pub use stop::StopInput;

pub mod legacy_input;
