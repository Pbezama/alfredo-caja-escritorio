// Sin consola en Windows (release).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    alfredo_caja_lib::run()
}
