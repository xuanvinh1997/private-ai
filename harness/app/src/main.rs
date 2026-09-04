// A Windows console window is only useful while debugging, so release builds do not open one.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    pai_app_lib::run()
}
