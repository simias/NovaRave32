mod main;
use crate::utils;

pub fn run_main_task(task_id: u32) -> ! {
    info!("Starting task {}", task_id);

    main::main();

    utils::shutdown(0)
}
