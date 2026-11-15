//! The Keeper Forge window. The Svelte page in `ui/` shows the screens; these commands do the work
//! by calling the same `ck-admin-core` and `ck-writer` code as the `ck-forge` command line.

mod hub;
mod profiles;
mod sticks;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(hub::HubState::default())
        .invoke_handler(tauri::generate_handler![
            profiles::new_profile,
            profiles::open_profile,
            profiles::save_profile,
            profiles::check_profile,
            sticks::list_disks,
            sticks::write_stick,
            hub::hub_sign_in,
            hub::hub_sign_out,
            hub::hub_status,
        ])
        .run(tauri::generate_context!())
        .expect("the Keeper Forge window failed to start");
}
