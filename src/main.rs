#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod quota;
#[cfg(windows)]
mod widget;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    widget::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("此原型需要 Windows。");
}
