mod idle;
mod main;

pub use idle::idle_main as idle_task;
pub use main::main as main_task;
