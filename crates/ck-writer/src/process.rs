//! Starting helper programs without a console window flashing on screen.

use std::process::Command;

/// Keeps a helper program from opening a console window. Forge is a window program, so without
/// this Windows shows a black console for every PowerShell, WSL or QEMU tool it starts. What the
/// program prints is still captured, so nothing else changes. Other systems need nothing.
pub fn without_console_window(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW, from the Windows process creation flags.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}
