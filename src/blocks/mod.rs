//! CommandBlock data model, `vte::Perform` ANSI/OSC 133/OSC 7 parser,
//! Alternate Screen handoff, and non-blocking Git branch detector.

pub mod git;
pub mod model;
mod osc;
pub mod parser;

pub use git::{AsyncGitDetector, GitBranchRequest, GitBranchResult, detect_git_branch_sync};
pub use model::{CommandBlock, CommandStatus, OutputLine, StyledSpan};
pub use parser::{EngineEvent, TerminalEngine};
