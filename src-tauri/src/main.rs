#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Keep the executable entry point minimal; all Tauri plugins, state, and
    // command registration live in the library so tests and the binary share
    // the same host setup.
    studypulse_client_lib::run();
}
