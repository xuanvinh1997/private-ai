// Cửa sổ console trên Windows chỉ hữu ích khi debug, nên bản release không mở nó.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    pai_app_lib::run()
}
