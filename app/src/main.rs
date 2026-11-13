//! The Keeper Forge window. The Svelte page in `ui/` shows the screens; these commands do the work
//! by calling the same `ck-admin-core` and `ck-writer` code as the `ck-forge` command line.

mod profiles;
mod sticks;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            profiles::new_profile,
            profiles::open_profile,
            profiles::save_profile,
            profiles::check_profile,
            sticks::build_image,
            sticks::list_disks,
            sticks::write_stick,
            sticks::push_registry,
        ])
        .run(tauri::generate_context!())
        .expect("the Keeper Forge window failed to start");
}
